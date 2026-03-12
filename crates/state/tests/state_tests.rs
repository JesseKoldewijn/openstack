//! Integration tests for state management: isolation, persistence, snapshot strategies, hooks.

#[cfg(test)]
mod state_tests {
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use openstack_config::{Config, Directories, SnapshotLoadStrategy, SnapshotSaveStrategy};
    use openstack_state::{
        AccountBundle, AccountRegionBundle, PersistableStore, StateFailureDiagnostic, StateHooks,
        StateManager,
    };
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn persistence_config(data_dir: &Path) -> Config {
        let dirs = Directories {
            data: data_dir.to_path_buf(),
            state: data_dir.join("state"),
            cache: data_dir.join("cache"),
            tmp: data_dir.join("tmp"),
            logs: data_dir.join("logs"),
            init: data_dir.join("init"),
            init_boot: data_dir.join("init/boot.d"),
            init_start: data_dir.join("init/start.d"),
            init_ready: data_dir.join("init/ready.d"),
            init_shutdown: data_dir.join("init/shutdown.d"),
            s3_objects: data_dir.join("s3/objects"),
            spool: data_dir.join("spool"),
        };
        Config {
            gateway_listen: vec!["0.0.0.0:4566".parse().unwrap()],
            persistence: true,
            services: openstack_config::ServicesConfig::from_env(),
            debug: false,
            log_level: openstack_config::LogLevel::Info,
            localstack_host: "localhost:4566".to_string(),
            allow_nonstandard_regions: false,
            cors: openstack_config::CorsConfig {
                disable_cors_headers: false,
                disable_cors_checks: false,
                extra_allowed_origins: vec![],
                extra_allowed_headers: vec![],
            },
            snapshot_save_strategy: SnapshotSaveStrategy::OnShutdown,
            snapshot_load_strategy: SnapshotLoadStrategy::OnStartup,
            snapshot_flush_interval: std::time::Duration::from_secs(15),
            dns_address: None,
            dns_port: 53,
            dns_resolve_ip: "127.0.0.1".to_string(),
            lambda_keepalive_ms: 600_000,
            lambda_remove_containers: true,
            bucket_marker_local: None,
            eager_service_loading: false,
            enable_config_updates: false,
            directories: dirs,
            body_spool_threshold_bytes: 1_048_576,
        }
    }

    // ─── A minimal serializable service store ────────────────────────────────

    #[derive(Default, Clone, Serialize, Deserialize)]
    struct Counter {
        value: u64,
    }

    /// A persistable store backed by an `AccountRegionBundle<Counter>`.
    struct CounterStore {
        bundle: AccountRegionBundle<Counter>,
    }

    impl CounterStore {
        fn new() -> Self {
            Self {
                bundle: AccountRegionBundle::new(),
            }
        }

        fn increment(&self, account: &str, region: &str) {
            self.bundle.get_or_create(account, region).value += 1;
        }

        fn get(&self, account: &str, region: &str) -> u64 {
            self.bundle
                .get(account, region)
                .map(|c| c.value)
                .unwrap_or(0)
        }
    }

    #[async_trait::async_trait]
    impl PersistableStore for CounterStore {
        fn service_name(&self) -> &str {
            "counter"
        }

        async fn save(&self, data_dir: &Path) -> Result<(), anyhow::Error> {
            for entry in self.bundle.iter() {
                let key = entry.key();
                let path =
                    openstack_state::state_path(data_dir, "counter", &key.account_id, &key.region);
                openstack_state::save_store(entry.value(), &path).await?;
            }
            Ok(())
        }

        async fn load(&self, data_dir: &Path) -> Result<(), anyhow::Error> {
            // Walk the counter state directory and load any existing snapshots.
            let base = data_dir.join("state").join("counter");
            if !base.exists() {
                return Ok(());
            }
            // account_id dirs
            let mut rd = tokio::fs::read_dir(&base).await?;
            while let Some(account_entry) = rd.next_entry().await? {
                let account_id = account_entry.file_name().to_string_lossy().to_string();
                let mut rd2 = tokio::fs::read_dir(account_entry.path()).await?;
                while let Some(region_entry) = rd2.next_entry().await? {
                    let region = region_entry.file_name().to_string_lossy().to_string();
                    let path = region_entry.path().join("store.json");
                    let counter: Counter = openstack_state::load_store(&path).await?;
                    *self.bundle.get_or_create(&account_id, &region) = counter;
                }
            }
            Ok(())
        }

        fn reset(&self) {
            self.bundle.clear();
        }
    }

    // ─── AccountRegionBundle isolation tests ─────────────────────────────────

    #[test]
    fn multi_account_isolation() {
        let bundle: AccountRegionBundle<Counter> = AccountRegionBundle::new();
        bundle.get_or_create("account-a", "us-east-1").value = 10;
        bundle.get_or_create("account-b", "us-east-1").value = 20;

        assert_eq!(bundle.get("account-a", "us-east-1").unwrap().value, 10);
        assert_eq!(bundle.get("account-b", "us-east-1").unwrap().value, 20);
    }

    #[test]
    fn multi_region_isolation() {
        let bundle: AccountRegionBundle<Counter> = AccountRegionBundle::new();
        bundle.get_or_create("account-a", "us-east-1").value = 1;
        bundle.get_or_create("account-a", "eu-west-1").value = 2;
        bundle.get_or_create("account-a", "ap-southeast-1").value = 3;

        assert_eq!(bundle.get("account-a", "us-east-1").unwrap().value, 1);
        assert_eq!(bundle.get("account-a", "eu-west-1").unwrap().value, 2);
        assert_eq!(bundle.get("account-a", "ap-southeast-1").unwrap().value, 3);
        // Non-existent combination returns None
        assert!(bundle.get("account-b", "us-east-1").is_none());
    }

    #[test]
    fn cross_region_attribute_via_account_bundle() {
        let bundle: AccountBundle<Counter> = AccountBundle::new();
        bundle.get_or_create("account-a").value = 100;
        bundle.get_or_create("account-b").value = 200;

        assert_eq!(bundle.get("account-a").unwrap().value, 100);
        assert_eq!(bundle.get("account-b").unwrap().value, 200);
        // Cross-region: same value regardless of (absent) region dimension
        *bundle.get_or_create("account-a") = Counter { value: 101 };
        assert_eq!(bundle.get("account-a").unwrap().value, 101);
    }

    #[test]
    fn bundle_clear() {
        let bundle: AccountRegionBundle<Counter> = AccountRegionBundle::new();
        bundle.get_or_create("a", "us-east-1").value = 5;
        assert_eq!(bundle.len(), 1);
        bundle.clear();
        assert_eq!(bundle.len(), 0);
    }

    // ─── Persistence round-trip ───────────────────────────────────────────────

    #[tokio::test]
    async fn persistence_round_trip() {
        let tmp = TempDir::new().unwrap();

        let store = CounterStore::new();
        store.increment("000000000000", "us-east-1");
        store.increment("000000000000", "us-east-1");
        store.increment("111111111111", "eu-west-1");

        assert_eq!(store.get("000000000000", "us-east-1"), 2);
        assert_eq!(store.get("111111111111", "eu-west-1"), 1);

        // Save
        store.save(tmp.path()).await.unwrap();

        // Clear and reload
        store.reset();
        assert_eq!(store.get("000000000000", "us-east-1"), 0);

        store.load(tmp.path()).await.unwrap();
        assert_eq!(store.get("000000000000", "us-east-1"), 2);
        assert_eq!(store.get("111111111111", "eu-west-1"), 1);
    }

    #[tokio::test]
    async fn load_missing_path_returns_default() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent").join("store.json");
        let counter: Counter = openstack_state::load_store(&path).await.unwrap();
        assert_eq!(counter.value, 0);
    }

    // ─── StateManager snapshot strategies ────────────────────────────────────

    #[tokio::test]
    async fn state_manager_on_shutdown_save_and_on_startup_load() {
        let tmp = TempDir::new().unwrap();
        let cfg = persistence_config(tmp.path());

        // --- First manager: write data, then save on shutdown ---
        {
            let store = Arc::new(CounterStore::new());
            store.increment("000000000000", "us-east-1");

            let mgr = StateManager::new(cfg.clone());
            mgr.register_store(Arc::clone(&store) as Arc<dyn PersistableStore>)
                .await;
            mgr.save_on_shutdown().await.unwrap();
        }

        // --- Second manager: load on startup ---
        {
            let store = Arc::new(CounterStore::new());
            let mgr = StateManager::new(cfg.clone());
            mgr.register_store(Arc::clone(&store) as Arc<dyn PersistableStore>)
                .await;
            mgr.load_on_startup().await.unwrap();

            assert_eq!(store.get("000000000000", "us-east-1"), 1);
        }
    }

    #[tokio::test]
    async fn state_manager_reset_clears_all_stores() {
        let tmp = TempDir::new().unwrap();
        let cfg = persistence_config(tmp.path());

        let store = Arc::new(CounterStore::new());
        store.increment("000000000000", "us-east-1");
        assert_eq!(store.get("000000000000", "us-east-1"), 1);

        let mgr = StateManager::new(cfg);
        mgr.register_store(Arc::clone(&store) as Arc<dyn PersistableStore>)
            .await;
        mgr.reset_all().await;

        assert_eq!(store.get("000000000000", "us-east-1"), 0);
    }

    #[tokio::test]
    async fn state_manager_no_persistence_is_noop() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = persistence_config(tmp.path());
        cfg.persistence = false;

        let store = Arc::new(CounterStore::new());
        store.increment("000000000000", "us-east-1");

        let mgr = StateManager::new(cfg);
        mgr.register_store(Arc::clone(&store) as Arc<dyn PersistableStore>)
            .await;

        // save_on_shutdown is a no-op when persistence is disabled
        mgr.save_on_shutdown().await.unwrap();
        // No files should have been written
        assert!(!tmp.path().join("state").exists());
    }

    // ─── Lifecycle hooks ──────────────────────────────────────────────────────

    struct CountingHooks {
        save_before: Arc<AtomicUsize>,
        save_after: Arc<AtomicUsize>,
        load_before: Arc<AtomicUsize>,
        load_after: Arc<AtomicUsize>,
        reset_before: Arc<AtomicUsize>,
        reset_after: Arc<AtomicUsize>,
        save_errors: Arc<AtomicUsize>,
        load_errors: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl StateHooks for CountingHooks {
        async fn on_before_state_save(&self) {
            self.save_before.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_after_state_save(&self) {
            self.save_after.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_before_state_load(&self) {
            self.load_before.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_after_state_load(&self) {
            self.load_after.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_before_state_reset(&self) {
            self.reset_before.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_after_state_reset(&self) {
            self.reset_after.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_state_save_error(&self, _diagnostic: &StateFailureDiagnostic) {
            self.save_errors.fetch_add(1, Ordering::Relaxed);
        }
        async fn on_state_load_error(&self, _diagnostic: &StateFailureDiagnostic) {
            self.load_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn hooks_are_invoked_on_save_load_reset() {
        let tmp = TempDir::new().unwrap();
        let cfg = persistence_config(tmp.path());

        let hooks = Arc::new(CountingHooks {
            save_before: Arc::new(AtomicUsize::new(0)),
            save_after: Arc::new(AtomicUsize::new(0)),
            load_before: Arc::new(AtomicUsize::new(0)),
            load_after: Arc::new(AtomicUsize::new(0)),
            reset_before: Arc::new(AtomicUsize::new(0)),
            reset_after: Arc::new(AtomicUsize::new(0)),
            save_errors: Arc::new(AtomicUsize::new(0)),
            load_errors: Arc::new(AtomicUsize::new(0)),
        });

        let save_before = Arc::clone(&hooks.save_before);
        let save_after = Arc::clone(&hooks.save_after);
        let load_before = Arc::clone(&hooks.load_before);
        let load_after = Arc::clone(&hooks.load_after);
        let reset_before = Arc::clone(&hooks.reset_before);
        let reset_after = Arc::clone(&hooks.reset_after);
        let save_errors = Arc::clone(&hooks.save_errors);
        let load_errors = Arc::clone(&hooks.load_errors);

        let store = Arc::new(CounterStore::new());
        let mgr = StateManager::with_hooks(cfg, hooks as Arc<dyn StateHooks>);
        mgr.register_store(Arc::clone(&store) as Arc<dyn PersistableStore>)
            .await;

        mgr.save_now().await.unwrap();
        assert_eq!(save_before.load(Ordering::Relaxed), 1);
        assert_eq!(save_after.load(Ordering::Relaxed), 1);

        mgr.load_now().await.unwrap();
        assert_eq!(load_before.load(Ordering::Relaxed), 1);
        assert_eq!(load_after.load(Ordering::Relaxed), 1);

        mgr.reset_all().await;
        assert_eq!(reset_before.load(Ordering::Relaxed), 1);
        assert_eq!(reset_after.load(Ordering::Relaxed), 1);
        assert_eq!(save_errors.load(Ordering::Relaxed), 0);
        assert_eq!(load_errors.load(Ordering::Relaxed), 0);
    }

    struct FailingLoadStore;

    #[async_trait::async_trait]
    impl PersistableStore for FailingLoadStore {
        fn service_name(&self) -> &str {
            "failing-load"
        }

        async fn save(&self, _data_dir: &Path) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn load(&self, _data_dir: &Path) -> Result<(), anyhow::Error> {
            Err(anyhow::anyhow!("boom"))
        }

        fn reset(&self) {}
    }

    #[tokio::test]
    async fn startup_load_fails_fast_for_unrecoverable_state() {
        let tmp = TempDir::new().unwrap();
        let cfg = persistence_config(tmp.path());

        let hooks = Arc::new(CountingHooks {
            save_before: Arc::new(AtomicUsize::new(0)),
            save_after: Arc::new(AtomicUsize::new(0)),
            load_before: Arc::new(AtomicUsize::new(0)),
            load_after: Arc::new(AtomicUsize::new(0)),
            reset_before: Arc::new(AtomicUsize::new(0)),
            reset_after: Arc::new(AtomicUsize::new(0)),
            save_errors: Arc::new(AtomicUsize::new(0)),
            load_errors: Arc::new(AtomicUsize::new(0)),
        });

        let load_errors = Arc::clone(&hooks.load_errors);
        let mgr = StateManager::with_hooks(cfg, hooks as Arc<dyn StateHooks>);
        mgr.register_store(Arc::new(FailingLoadStore) as Arc<dyn PersistableStore>)
            .await;

        let err = mgr.load_on_startup().await.expect_err("must fail fast");
        assert!(
            err.to_string().contains("startup_state_load_failed"),
            "unexpected error: {err}"
        );
        assert_eq!(load_errors.load(Ordering::Relaxed), 1);
    }
}
