use super::*;
use crate::config::Constrained;
use crate::config::NetworkProxySpec;
use crate::config::test_config;
use crate::test_support;
use codex_network_proxy::NetworkProxyConfig;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GuardianAssessmentStatus;
use codex_protocol::protocol::ReviewDecision;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn shell_request(id: &str) -> GuardianApprovalRequest {
    GuardianApprovalRequest::Shell {
        id: id.to_string(),
        command: vec!["git".to_string(), "push".to_string()],
        cwd: PathBuf::from("/repo"),
        sandbox_permissions: crate::sandboxing::SandboxPermissions::UseDefault,
        additional_permissions: None,
        justification: Some("push the reviewed branch".to_string()),
    }
}

fn apply_patch_request(id: &str) -> GuardianApprovalRequest {
    GuardianApprovalRequest::ApplyPatch {
        id: id.to_string(),
        cwd: PathBuf::from("/tmp"),
        files: vec![
            codex_utils_absolute_path::AbsolutePathBuf::try_from("/tmp/guardian.txt")
                .expect("absolute path"),
        ],
        change_count: 1usize,
        patch: "*** Begin Patch\n*** Update File: guardian.txt\n@@\n+hello\n*** End Patch"
            .to_string(),
    }
}

async fn make_guardian_review_context(
    base_url: String,
) -> (Arc<crate::codex::Session>, Arc<crate::codex::TurnContext>) {
    let (mut session, mut turn) = crate::codex::make_session_and_context().await;
    let mut config = (*turn.config).clone();
    config.model_provider.base_url = Some(base_url);
    let config = Arc::new(config);
    let models_manager = Arc::new(test_support::models_manager_with_provider(
        config.codex_home.clone(),
        Arc::clone(&session.services.auth_manager),
        config.model_provider.clone(),
    ));
    session.services.models_manager = models_manager;
    turn.config = Arc::clone(&config);
    turn.provider = config.model_provider.clone();
    (Arc::new(session), Arc::new(turn))
}

#[test]
fn build_guardian_transcript_keeps_original_numbering() {
    let entries = [
        GuardianTranscriptEntry {
            kind: GuardianTranscriptEntryKind::User,
            text: "first".to_string(),
        },
        GuardianTranscriptEntry {
            kind: GuardianTranscriptEntryKind::Assistant,
            text: "second".to_string(),
        },
        GuardianTranscriptEntry {
            kind: GuardianTranscriptEntryKind::Assistant,
            text: "third".to_string(),
        },
    ];

    let (transcript, omission) = render_guardian_transcript_entries(&entries[..2]);

    assert_eq!(
        transcript,
        vec![
            "[1] user: first".to_string(),
            "[2] assistant: second".to_string(),
        ]
    );
    assert!(omission.is_none());
}

#[test]
fn collect_guardian_transcript_entries_skips_contextual_user_messages() {
    let items = vec![
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "<environment_context>\n<cwd>/tmp</cwd>\n</environment_context>".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        },
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: "hello".to_string(),
            }],
            end_turn: None,
            phase: None,
            thought_signature: None,
        },
    ];

    let entries = collect_guardian_transcript_entries(&items);

    assert_eq!(
        entries,
        vec![GuardianTranscriptEntry {
            kind: GuardianTranscriptEntryKind::Assistant,
            text: "hello".to_string(),
        }],
    );
}

#[test]
fn guardian_assessment_action_value_redacts_apply_patch_patch_text() {
    assert_eq!(
        guardian_assessment_action_value(&apply_patch_request("patch-1")),
        serde_json::json!({
            "tool": "apply_patch",
            "cwd": "/tmp",
            "files": ["/tmp/guardian.txt"],
            "change_count": 1,
        })
    );
}

#[test]
fn parse_guardian_assessment_extracts_embedded_json() {
    let parsed = parse_guardian_assessment(Some(
        "preface {\"risk_level\":\"medium\",\"risk_score\":42,\"rationale\":\"ok\",\"evidence\":[]}",
    ))
    .expect("guardian assessment");

    assert_eq!(parsed.risk_score, 42);
}

#[test]
fn guardian_subagent_config_preserves_parent_network_proxy() {
    let mut parent_config = test_config();
    let network =
        NetworkProxySpec::from_config_and_constraints(NetworkProxyConfig::default(), None)
            .expect("network proxy config");
    parent_config.permissions.network = Some(network);

    let guardian_config =
        build_guardian_subagent_config(&parent_config, None, "gpt-5.1", None).expect("config");

    assert_eq!(
        guardian_config.permissions.network,
        parent_config.permissions.network
    );
}

#[tokio::test]
async fn cancelled_guardian_review_emits_terminal_abort_without_warning() {
    let (session, turn, rx) = crate::codex::make_session_and_context_with_rx().await;
    let cancel_token = CancellationToken::new();
    cancel_token.cancel();

    let decision = review_approval_request_with_cancel(
        &session,
        &turn,
        apply_patch_request("patch-1"),
        None,
        cancel_token,
    )
    .await;

    assert_eq!(decision, ReviewDecision::Abort);

    let mut guardian_statuses = Vec::new();
    let mut warnings = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event.msg {
            EventMsg::GuardianAssessment(event) => guardian_statuses.push(event.status),
            EventMsg::Warning(event) => warnings.push(event.message),
            _ => {}
        }
    }

    assert_eq!(
        guardian_statuses,
        vec![
            GuardianAssessmentStatus::InProgress,
            GuardianAssessmentStatus::Aborted,
        ]
    );
    assert!(warnings.is_empty());
}

#[tokio::test]
async fn routes_approval_to_guardian_requires_on_request_guardian_reviewer() {
    let (_session, mut turn) = crate::codex::make_session_and_context().await;
    let mut config = (*turn.config).clone();
    config.approvals_reviewer = ApprovalsReviewer::User;
    turn.config = Arc::new(config.clone());
    turn.approval_policy = Constrained::allow_only(AskForApproval::OnRequest);

    assert!(!routes_approval_to_guardian(&turn));

    config.approvals_reviewer = ApprovalsReviewer::GuardianSubagent;
    turn.config = Arc::new(config);

    assert!(routes_approval_to_guardian(&turn));

    turn.approval_policy = Constrained::allow_only(AskForApproval::Never);

    assert!(!routes_approval_to_guardian(&turn));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn low_risk_guardian_review_is_approved() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-guardian"),
            ev_assistant_message(
                "msg-guardian",
                r#"{"risk_level":"low","risk_score":42,"rationale":"safe","evidence":[]}"#,
            ),
            ev_completed("resp-guardian"),
        ]),
    )
    .await;

    let (session, turn) = make_guardian_review_context(format!("{}/v1", server.uri())).await;
    let decision = review_approval_request(&session, &turn, shell_request("shell-1"), None).await;

    assert_eq!(decision, ReviewDecision::Approved);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn high_risk_guardian_review_is_denied() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-guardian"),
            ev_assistant_message(
                "msg-guardian",
                r#"{"risk_level":"high","risk_score":90,"rationale":"destructive","evidence":[]}"#,
            ),
            ev_completed("resp-guardian"),
        ]),
    )
    .await;

    let (session, turn) = make_guardian_review_context(format!("{}/v1", server.uri())).await;
    let decision = review_approval_request(&session, &turn, shell_request("shell-1"), None).await;

    assert_eq!(decision, ReviewDecision::Denied);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_guardian_review_fails_closed() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-guardian"),
            ev_assistant_message("msg-guardian", "not json"),
            ev_completed("resp-guardian"),
        ]),
    )
    .await;

    let (session, turn) = make_guardian_review_context(format!("{}/v1", server.uri())).await;
    let decision = review_approval_request(&session, &turn, shell_request("shell-1"), None).await;

    assert_eq!(decision, ReviewDecision::Denied);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn guardian_review_transport_error_fails_closed() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let (session, turn) = make_guardian_review_context("http://127.0.0.1:1/v1".to_string()).await;
    let decision = review_approval_request(&session, &turn, shell_request("shell-1"), None).await;

    assert_eq!(decision, ReviewDecision::Denied);
    Ok(())
}
