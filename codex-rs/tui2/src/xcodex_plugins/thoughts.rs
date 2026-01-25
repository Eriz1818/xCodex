use crate::chatwidget::ChatWidget;

pub(crate) fn handle(chat: &mut ChatWidget, rest: &str) -> bool {
    let args: Vec<&str> = rest.split_whitespace().collect();
    let next_hide = match args.as_slice() {
        [] => Some(!chat.hide_agent_reasoning()),
        [arg] => match arg.to_ascii_lowercase().as_str() {
            "on" | "show" | "true" => Some(false),
            "off" | "hide" | "false" => Some(true),
            "toggle" => Some(!chat.hide_agent_reasoning()),
            "status" => None,
            _ => {
                chat.add_info_message("Usage: /thoughts [on|off|toggle|status]".to_string(), None);
                return true;
            }
        },
        _ => {
            chat.add_info_message("Usage: /thoughts [on|off|toggle|status]".to_string(), None);
            return true;
        }
    };

    if let Some(hide) = next_hide {
        chat.apply_hide_agent_reasoning(hide);
        let status = if hide { "hidden" } else { "shown" };
        chat.add_info_message(format!("Thoughts {status}."), None);
    } else {
        let status = if chat.hide_agent_reasoning() {
            "hidden"
        } else {
            "shown"
        };
        chat.add_info_message(format!("Thoughts are currently {status}."), None);
    }
    true
}
