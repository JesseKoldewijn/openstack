use crate::guided_manifest::GuidedManifest;
use crate::history::InteractionEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    pub title: String,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDetailLayout {
    pub service: String,
    pub guided_panel: PanelState,
    pub raw_panel: PanelState,
    pub history_panel: PanelState,
    pub selected_flow_id: Option<String>,
    pub history_count: usize,
}

/// Constructs a ServiceDetailLayout for a service from an optional GuidedManifest and interaction history.
///
/// The returned layout sets `guided_panel.visible` to true when a manifest is provided, `selected_flow_id` to the
/// id of the manifest's first flow if present, and `history_count` to the length of `history`.
///
/// # Examples
///
/// ```
/// use crate::service_detail::build_service_detail_layout;
/// use crate::guided_manifest::GuidedManifest;
/// use crate::history::InteractionEntry;
///
/// let layout = build_service_detail_layout("s3", None::<&GuidedManifest>, &[] as &[InteractionEntry]);
/// assert_eq!(layout.service, "s3");
/// assert_eq!(layout.guided_panel.visible, false);
/// assert_eq!(layout.history_count, 0);
/// ```
pub fn build_service_detail_layout(
    service: &str,
    manifest: Option<&GuidedManifest>,
    history: &[InteractionEntry],
) -> ServiceDetailLayout {
    let selected_flow_id = manifest
        .and_then(|m| m.flows.first())
        .map(|flow| flow.id.clone());

    ServiceDetailLayout {
        service: service.to_string(),
        guided_panel: PanelState {
            title: "Guided Flow".to_string(),
            visible: manifest.is_some(),
        },
        raw_panel: PanelState {
            title: "Raw Interaction".to_string(),
            visible: true,
        },
        history_panel: PanelState {
            title: "Interaction History".to_string(),
            visible: true,
        },
        selected_flow_id,
        history_count: history.len(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::api::RawRequest;
    use crate::guided_manifest::{GuidedFlow, GuidedStep, NormalizedOperation, ProtocolClass};

    #[test]
    fn service_detail_enables_guided_panel_when_manifest_exists() {
        let manifest = GuidedManifest {
            schema_version: "1.2".to_string(),
            service: "s3".to_string(),
            protocol: ProtocolClass::RestXml,
            flows: vec![GuidedFlow {
                id: "l1-basic".to_string(),
                level: "L1".to_string(),
                steps: vec![GuidedStep {
                    id: "step-1".to_string(),
                    title: "Step".to_string(),
                    operation: NormalizedOperation {
                        method: "GET".to_string(),
                        path: "/".to_string(),
                        headers: HashMap::new(),
                        query: HashMap::new(),
                        body: None,
                    },
                    assertions: vec![],
                    captures: vec![],
                    error_guidance: None,
                }],
                cleanup: vec![],
            }],
        };

        let layout = build_service_detail_layout("s3", Some(&manifest), &[]);
        assert!(layout.guided_panel.visible);
        assert_eq!(layout.selected_flow_id.as_deref(), Some("l1-basic"));
    }

    #[test]
    fn service_detail_tracks_history_count() {
        let history = vec![InteractionEntry {
            id: 1,
            timestamp_unix_ms: 0,
            service: "s3".to_string(),
            status: 200,
            request: RawRequest {
                method: "GET".to_string(),
                path: "/_localstack/health".to_string(),
                query: HashMap::new(),
                headers: HashMap::new(),
                body: None,
            },
        }];

        let layout = build_service_detail_layout("s3", None, &history);
        assert!(!layout.guided_panel.visible);
        assert!(layout.raw_panel.visible);
        assert!(layout.history_panel.visible);
        assert_eq!(layout.history_count, 1);
    }
}
