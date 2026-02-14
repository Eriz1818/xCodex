use codex_protocol::protocol::ExclusionCounts;
use codex_protocol::protocol::ExclusionLayerCounts;
use codex_protocol::protocol::ExclusionSourceCounts;
use codex_protocol::protocol::ExclusionSummaryEvent;
use codex_protocol::protocol::ExclusionToolCount;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(crate) struct ExclusionTurnCounters {
    layers: ExclusionLayerCounts,
    sources: ExclusionSourceCounts,
    per_tool: BTreeMap<String, ExclusionCounts>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExclusionLayer {
    Layer1InputGuards,
    Layer2OutputSanitization,
    Layer3SendFirewall,
    Layer4RequestInterceptor,
    Layer5HookSanitization,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExclusionSource {
    Filesystem,
    Mcp,
    Shell,
    Prompt,
    Other,
}

impl ExclusionTurnCounters {
    pub(crate) fn record(
        &mut self,
        layer: ExclusionLayer,
        source: ExclusionSource,
        tool_name: &str,
        redacted: bool,
        blocked: bool,
    ) {
        let delta_redacted = i64::from(redacted);
        let delta_blocked = i64::from(blocked);

        if delta_redacted == 0 && delta_blocked == 0 {
            return;
        }

        let counts = self.layer_counts_mut(layer);
        counts.redacted += delta_redacted;
        counts.blocked += delta_blocked;

        let counts = self.source_counts_mut(source);
        counts.redacted += delta_redacted;
        counts.blocked += delta_blocked;

        let entry = self.per_tool.entry(tool_name.to_string()).or_default();
        entry.redacted += delta_redacted;
        entry.blocked += delta_blocked;
    }

    pub(crate) fn snapshot(&self) -> Option<ExclusionSummaryEvent> {
        let total_redacted = self.sources.filesystem.redacted
            + self.sources.mcp.redacted
            + self.sources.shell.redacted
            + self.sources.prompt.redacted
            + self.sources.other.redacted;
        let total_blocked = self.sources.filesystem.blocked
            + self.sources.mcp.blocked
            + self.sources.shell.blocked
            + self.sources.prompt.blocked
            + self.sources.other.blocked;

        if total_redacted == 0 && total_blocked == 0 {
            return None;
        }

        let per_tool = self
            .per_tool
            .iter()
            .map(|(tool_name, counts)| ExclusionToolCount {
                tool_name: tool_name.clone(),
                counts: counts.clone(),
            })
            .collect::<Vec<_>>();

        Some(ExclusionSummaryEvent {
            total_redacted,
            total_blocked,
            layers: self.layers.clone(),
            sources: self.sources.clone(),
            per_tool,
        })
    }

    fn layer_counts_mut(&mut self, layer: ExclusionLayer) -> &mut ExclusionCounts {
        match layer {
            ExclusionLayer::Layer1InputGuards => &mut self.layers.layer1_input_guards,
            ExclusionLayer::Layer2OutputSanitization => &mut self.layers.layer2_output_sanitization,
            ExclusionLayer::Layer3SendFirewall => &mut self.layers.layer3_send_firewall,
            ExclusionLayer::Layer4RequestInterceptor => &mut self.layers.layer4_request_interceptor,
            ExclusionLayer::Layer5HookSanitization => &mut self.layers.layer5_hook_sanitization,
        }
    }

    fn source_counts_mut(&mut self, source: ExclusionSource) -> &mut ExclusionCounts {
        match source {
            ExclusionSource::Filesystem => &mut self.sources.filesystem,
            ExclusionSource::Mcp => &mut self.sources.mcp,
            ExclusionSource::Shell => &mut self.sources.shell,
            ExclusionSource::Prompt => &mut self.sources.prompt,
            ExclusionSource::Other => &mut self.sources.other,
        }
    }
}
