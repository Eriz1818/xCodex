use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ExclusionSummaryEvent {
    #[ts(type = "number")]
    pub total_redacted: i64,
    #[ts(type = "number")]
    pub total_blocked: i64,

    pub layers: ExclusionLayerCounts,
    pub sources: ExclusionSourceCounts,

    #[serde(default)]
    pub per_tool: Vec<ExclusionToolCount>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ExclusionLayerCounts {
    pub layer1_input_guards: ExclusionCounts,
    pub layer2_output_sanitization: ExclusionCounts,
    pub layer3_send_firewall: ExclusionCounts,
    pub layer4_request_interceptor: ExclusionCounts,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ExclusionSourceCounts {
    pub filesystem: ExclusionCounts,
    pub mcp: ExclusionCounts,
    pub shell: ExclusionCounts,
    pub prompt: ExclusionCounts,
    pub other: ExclusionCounts,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ExclusionCounts {
    #[ts(type = "number")]
    pub redacted: i64,
    #[ts(type = "number")]
    pub blocked: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
pub struct ExclusionToolCount {
    pub tool_name: String,
    pub counts: ExclusionCounts,
}

#[cfg(test)]
mod tests {
    use super::ExclusionCounts;
    use super::ExclusionLayerCounts;
    use super::ExclusionSourceCounts;
    use super::ExclusionSummaryEvent;
    use super::ExclusionToolCount;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn exclusion_summary_defaults_per_tool_when_omitted() {
        let summary = serde_json::from_value::<ExclusionSummaryEvent>(json!({
            "total_redacted": 3,
            "total_blocked": 2,
            "layers": {
                "layer1_input_guards": {"redacted": 1, "blocked": 0},
                "layer2_output_sanitization": {"redacted": 1, "blocked": 1},
                "layer3_send_firewall": {"redacted": 1, "blocked": 1},
                "layer4_request_interceptor": {"redacted": 0, "blocked": 0}
            },
            "sources": {
                "filesystem": {"redacted": 1, "blocked": 0},
                "mcp": {"redacted": 1, "blocked": 1},
                "shell": {"redacted": 1, "blocked": 1},
                "prompt": {"redacted": 0, "blocked": 0},
                "other": {"redacted": 0, "blocked": 0}
            }
        }))
        .expect("summary should deserialize");

        assert_eq!(
            summary,
            ExclusionSummaryEvent {
                total_redacted: 3,
                total_blocked: 2,
                layers: ExclusionLayerCounts {
                    layer1_input_guards: ExclusionCounts {
                        redacted: 1,
                        blocked: 0,
                    },
                    layer2_output_sanitization: ExclusionCounts {
                        redacted: 1,
                        blocked: 1,
                    },
                    layer3_send_firewall: ExclusionCounts {
                        redacted: 1,
                        blocked: 1,
                    },
                    layer4_request_interceptor: ExclusionCounts {
                        redacted: 0,
                        blocked: 0,
                    },
                },
                sources: ExclusionSourceCounts {
                    filesystem: ExclusionCounts {
                        redacted: 1,
                        blocked: 0,
                    },
                    mcp: ExclusionCounts {
                        redacted: 1,
                        blocked: 1,
                    },
                    shell: ExclusionCounts {
                        redacted: 1,
                        blocked: 1,
                    },
                    prompt: ExclusionCounts {
                        redacted: 0,
                        blocked: 0,
                    },
                    other: ExclusionCounts {
                        redacted: 0,
                        blocked: 0,
                    },
                },
                per_tool: vec![],
            }
        );
    }

    #[test]
    fn exclusion_summary_round_trips_with_per_tool() {
        let summary = ExclusionSummaryEvent {
            total_redacted: 6,
            total_blocked: 3,
            layers: ExclusionLayerCounts::default(),
            sources: ExclusionSourceCounts::default(),
            per_tool: vec![ExclusionToolCount {
                tool_name: "shell".to_string(),
                counts: ExclusionCounts {
                    redacted: 4,
                    blocked: 2,
                },
            }],
        };

        let value = serde_json::to_value(&summary).expect("summary should serialize");
        let parsed = serde_json::from_value::<ExclusionSummaryEvent>(value)
            .expect("summary should deserialize");

        assert_eq!(parsed, summary);
    }
}
