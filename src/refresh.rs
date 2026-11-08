use anyhow::{Context, Result};
use serde_json::Value;

use crate::reward::{self, AuthenticatedSession};

pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    RenewToken,
    Relogin,
}

pub(crate) fn is_expired(status: u16, body: &str) -> bool {
    status == 405
        || serde_json::from_str::<Value>(body)
            .is_ok_and(|reply| reply["result"] != true && reply["code"] == 2)
}

fn next_step(previous: Option<Step>, expired: bool) -> Option<Step> {
    if !expired {
        return None;
    }
    match previous {
        None => Some(Step::RenewToken),
        Some(Step::RenewToken) => Some(Step::Relogin),
        Some(Step::Relogin) => None,
    }
}

pub(crate) fn execute(
    session: &mut AuthenticatedSession,
    mut send: impl FnMut(&AuthenticatedSession) -> Result<Reply>,
) -> Result<Reply> {
    let mut reply = send(session)?;
    let mut step = None;
    while let Some(next) = next_step(step, is_expired(reply.status, &reply.body)) {
        step = Some(next);
        match next {
            Step::RenewToken => {
                if reward::renew_token(session).is_err() {
                    continue;
                }
            }
            Step::Relogin => {
                reward::relogin(session).context("shop session expired and re-login failed")?;
            }
        }
        reply = send(session)?;
    }
    Ok(reply)
}

#[test]
fn http_405_and_code_2_mean_expired() {
    assert!(is_expired(405, ""));
    assert!(is_expired(200, r#"{"result":false,"code":2,"msg":"x"}"#));
}

#[test]
fn other_replies_are_not_expired() {
    assert!(!is_expired(200, r#"{"result":true,"code":2,"data":{}}"#));
    assert!(!is_expired(200, r#"{"result":false,"code":3,"msg":"x"}"#));
    assert!(!is_expired(500, "upstream error"));
    assert!(!is_expired(200, "not json"));
}

#[test]
fn escalates_from_token_renewal_to_relogin_once() {
    assert_eq!(next_step(None, false), None);
    assert_eq!(next_step(None, true), Some(Step::RenewToken));
    assert_eq!(next_step(Some(Step::RenewToken), true), Some(Step::Relogin));
    assert_eq!(next_step(Some(Step::RenewToken), false), None);
    assert_eq!(next_step(Some(Step::Relogin), true), None);
}
