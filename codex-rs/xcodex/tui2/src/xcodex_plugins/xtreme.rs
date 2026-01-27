use crate::bottom_pane::StatusMenuTab;
use crate::chatwidget::ChatWidget;

pub(crate) fn handle(chat: &mut ChatWidget, rest: &str) -> bool {
    if !rest.trim().is_empty() {
        chat.add_info_message("Usage: /xtreme".to_string(), None);
        return true;
    }

    chat.open_status_menu_view(StatusMenuTab::Tools);
    true
}
