/// ARN generation utilities.
///
/// Builds AWS ARN strings from request context components (account_id, region, service, resource).
/// Format: arn:aws:{service}:{region}:{account_id}:{resource_type}/{resource_id}
///
/// # Build an ARN for a resource
///
/// # Arguments
/// * `service` - AWS service name (e.g., "sqs", "sns", "lambda")
/// * `region` - AWS region (e.g., "us-east-1"). Pass "" for global services.
/// * `account_id` - 12-digit AWS account ID
/// * `resource` - Resource identifier (e.g., "queue/my-queue", "function:my-fn", "role/MyRole")
///
/// # Example
/// ```
/// use openstack_service_framework::arn::make_arn;
/// let arn = make_arn("sqs", "us-east-1", "000000000000", "queue/my-queue");
/// assert_eq!(arn, "arn:aws:sqs:us-east-1:000000000000:queue/my-queue");
/// ```
pub fn make_arn(service: &str, region: &str, account_id: &str, resource: &str) -> String {
    format!("arn:aws:{service}:{region}:{account_id}:{resource}")
}

/// Build an ARN for a named resource with a given resource type.
///
/// # Example
/// ```
/// use openstack_service_framework::arn::make_arn_typed;
/// let arn = make_arn_typed("lambda", "us-east-1", "000000000000", "function", "my-fn");
/// assert_eq!(arn, "arn:aws:lambda:us-east-1:000000000000:function:my-fn");
/// ```
pub fn make_arn_typed(
    service: &str,
    region: &str,
    account_id: &str,
    resource_type: &str,
    resource_name: &str,
) -> String {
    format!("arn:aws:{service}:{region}:{account_id}:{resource_type}:{resource_name}")
}

/// Build an ARN using the request context from `RequestContext`.
pub fn arn_from_ctx(ctx: &crate::traits::RequestContext, resource: &str) -> String {
    make_arn(&ctx.service, &ctx.region, &ctx.account_id, resource)
}

/// Build a typed ARN using the request context.
pub fn arn_typed_from_ctx(
    ctx: &crate::traits::RequestContext,
    resource_type: &str,
    resource_name: &str,
) -> String {
    make_arn_typed(
        &ctx.service,
        &ctx.region,
        &ctx.account_id,
        resource_type,
        resource_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_arn() {
        let arn = make_arn("sqs", "us-east-1", "000000000000", "my-queue");
        assert_eq!(arn, "arn:aws:sqs:us-east-1:000000000000:my-queue");
    }

    #[test]
    fn test_make_arn_typed() {
        let arn = make_arn_typed("lambda", "us-east-1", "000000000000", "function", "my-fn");
        assert_eq!(arn, "arn:aws:lambda:us-east-1:000000000000:function:my-fn");
    }

    #[test]
    fn test_make_arn_global_service() {
        // IAM is global — no region
        let arn = make_arn("iam", "", "000000000000", "role/MyRole");
        assert_eq!(arn, "arn:aws:iam::000000000000:role/MyRole");
    }

    #[test]
    fn test_make_arn_slash_resource() {
        let arn = make_arn("s3", "", "", "my-bucket");
        assert_eq!(arn, "arn:aws:s3:::my-bucket");
    }
}
