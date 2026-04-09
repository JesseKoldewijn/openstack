const https = require("node:https");

function repoParts(context) {
  const repo =
    process.env.GITHUB_REPOSITORY ||
    context?.env?.GITHUB_REPOSITORY ||
    "";

  const [owner, name] = repo.split("/");
  if (!owner || !name) {
    throw new Error("Unable to determine GITHUB_REPOSITORY for GHCR release note enrichment");
  }

  return { owner, name };
}

function buildGhcrSection(context) {
  const { owner, name } = repoParts(context);
  const image = `ghcr.io/${owner.toLowerCase()}/${name.toLowerCase()}`;
  const packageUrl = `https://github.com/${owner}/${name}/pkgs/container/${name}`;
  const branchName = context?.branch?.name || "";
  const gitHead = context?.nextRelease?.gitHead || process.env.GITHUB_SHA || "";
  const shortSha = gitHead ? gitHead.slice(0, 7) : "";

  if (branchName === "develop") {
    const lines = [
      "## Container image",
      `- GHCR package: ${packageUrl}`,
      `- Pull: \`docker pull ${image}:beta\``,
    ];

    if (shortSha) {
      lines.push(`- Commit-scoped image: \`docker pull ${image}:beta-${shortSha}\``);
    }

    return lines.join("\n");
  }

  return [
    "## Container image",
    `- GHCR package: ${packageUrl}`,
    `- Pull: \`docker pull ${image}:stable\``,
    `- Also available as: \`docker pull ${image}:latest\``,
  ].join("\n");
}

function githubRequest(method, path, token, body) {
  return new Promise((resolve, reject) => {
    const req = https.request(
      {
        hostname: "api.github.com",
        path,
        method,
        headers: {
          Authorization: `Bearer ${token}`,
          Accept: "application/vnd.github+json",
          "User-Agent": "openstack-semantic-release-ghcr-links",
          "X-GitHub-Api-Version": "2022-11-28",
          ...(body ? { "Content-Type": "application/json" } : {}),
        },
      },
      (res) => {
        let data = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => {
          data += chunk;
        });
        res.on("end", () => {
          const status = res.statusCode || 0;
          if (status >= 200 && status < 300) {
            resolve(data ? JSON.parse(data) : {});
            return;
          }

          reject(
            new Error(
              `GitHub API ${method} ${path} failed with ${status}: ${data}`,
            ),
          );
        });
      },
    );

    req.on("error", reject);

    if (body) {
      req.write(JSON.stringify(body));
    }

    req.end();
  });
}

async function success(_pluginConfig, context) {
  const token = process.env.GITHUB_TOKEN || context?.env?.GITHUB_TOKEN;
  if (!token) {
    context.logger.log("Skipping GHCR release note enrichment because GITHUB_TOKEN is unavailable");
    return;
  }

  const { owner, name } = repoParts(context);
  const tag = context?.nextRelease?.gitTag;
  const notes = context?.nextRelease?.notes || "";
  if (!tag) {
    context.logger.log("Skipping GHCR release note enrichment because no release tag was produced");
    return;
  }

  const ghcrSection = buildGhcrSection(context);
  const release = await githubRequest(
    "GET",
    `/repos/${owner}/${name}/releases/tags/${encodeURIComponent(tag)}`,
    token,
  );

  const currentBody = release.body || "";
  if (currentBody.includes("## Container image")) {
    context.logger.log("GitHub release notes already include GHCR information; skipping update");
    return;
  }

  const newBody = `${notes.trim()}\n\n${ghcrSection}`;
  await githubRequest(
    "PATCH",
    `/repos/${owner}/${name}/releases/${release.id}`,
    token,
    { body: newBody },
  );

  context.logger.log(`Updated GitHub release notes for ${tag} with GHCR image links`);
}

module.exports = {
  success,
  buildGhcrSection,
};
