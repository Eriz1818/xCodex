use std::collections::HashSet;

use rmcp::model::CreateElicitationRequestParams;

pub(crate) fn elicitation_message(request: &CreateElicitationRequestParams) -> &str {
    match request {
        CreateElicitationRequestParams::FormElicitationParams { message, .. }
        | CreateElicitationRequestParams::UrlElicitationParams { message, .. } => message,
    }
}

pub(crate) fn should_accept_elicitation_message(
    message: &str,
    elicitations_to_accept: &HashSet<String>,
    accept_all_elicitations: bool,
) -> bool {
    accept_all_elicitations || elicitations_to_accept.contains(message)
}

pub(crate) fn should_accept_elicitation_request(
    request: &CreateElicitationRequestParams,
    elicitations_to_accept: &HashSet<String>,
    accept_all_elicitations: bool,
) -> bool {
    should_accept_elicitation_message(
        elicitation_message(request),
        elicitations_to_accept,
        accept_all_elicitations,
    )
}

#[cfg(test)]
mod tests {
    use super::should_accept_elicitation_message;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn accepts_any_message_when_accept_all_is_true() {
        let elicitations_to_accept: HashSet<String> = HashSet::new();
        let accept = should_accept_elicitation_message(
            "Allow agent to run `git init .` in `/tmp`?",
            &elicitations_to_accept,
            true,
        );
        assert_eq!(accept, true);
    }

    #[test]
    fn accepts_only_whitelisted_message_when_accept_all_is_false() {
        let mut elicitations_to_accept: HashSet<String> = HashSet::new();
        elicitations_to_accept.insert("message-a".to_string());

        let accept_match =
            should_accept_elicitation_message("message-a", &elicitations_to_accept, false);
        let accept_miss =
            should_accept_elicitation_message("message-b", &elicitations_to_accept, false);

        assert_eq!(accept_match, true);
        assert_eq!(accept_miss, false);
    }
}
