use openstack_studio_ui::{
    GuidedWorkflow, GuidedWorkflowKind, ServiceCatalog, StudioServicesResponse,
    models::ServiceEntry,
};

#[test]
fn catalog_support_tier_filtering_and_lookup() {
    let response = StudioServicesResponse {
        services: vec![
            ServiceEntry {
                name: "s3".to_string(),
                status: "available".to_string(),
                support_tier: "guided".to_string(),
            },
            ServiceEntry {
                name: "events".to_string(),
                status: "available".to_string(),
                support_tier: "raw".to_string(),
            },
        ],
    };
    let catalog = ServiceCatalog::from_response(response);

    assert_eq!(catalog.all().len(), 2);
    assert_eq!(catalog.by_tier("guided").count(), 1);
    assert!(catalog.by_name("s3").is_some());
    assert!(catalog.by_name("missing").is_none());
}

#[test]
fn guided_workflow_templates_generate_expected_steps() {
    let s3 = GuidedWorkflow::s3_basic("bucket-a", "key-a", "body-a");
    assert_eq!(s3.kind, GuidedWorkflowKind::S3);
    assert_eq!(s3.steps.len(), 2);
    assert_eq!(s3.steps[0].title, "Create bucket");

    let sqs = GuidedWorkflow::sqs_basic("queue-a", "hello");
    assert_eq!(sqs.kind, GuidedWorkflowKind::Sqs);
    assert_eq!(sqs.steps.len(), 2);
    assert!(
        sqs.steps[0]
            .request
            .body
            .as_ref()
            .unwrap()
            .contains("CreateQueue")
    );
}
