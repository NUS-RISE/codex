use super::*;
use codex_app_server_protocol::RawResponseCompletedNotification;
use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
use codex_app_server_protocol::TokenUsageBreakdown;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn failed_turn_does_not_overwrite_output_last_message_file() {
    let tempdir = tempdir().expect("create tempdir");
    let output_path = tempdir.path().join("last-message.txt");
    std::fs::write(&output_path, "keep existing contents").expect("seed output file");

    let mut processor = EventProcessorWithJsonOutput::new(Some(output_path.clone()));

    let collected = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::AgentMessage {
                id: "msg-1".to_string(),
                text: "partial answer".to_string(),
                phase: None,
                memory_citation: None,
                delivery: None,
                questions: None,
            },
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 0,
        },
    ));

    assert_eq!(collected.status, CodexStatus::Running);
    assert_eq!(processor.final_message(), Some("partial answer"));

    let status = processor.process_server_notification(ServerNotification::TurnCompleted(
        codex_app_server_protocol::TurnCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn: codex_app_server_protocol::Turn {
                id: "turn-1".to_string(),
                items_view: codex_app_server_protocol::TurnItemsView::Full,
                items: Vec::new(),
                status: TurnStatus::Failed,
                error: Some(codex_app_server_protocol::TurnError {
                    misalignment: None,
                    message: "turn failed".to_string(),
                    additional_details: None,
                    codex_error_info: None,
                }),
                started_at: None,
                completed_at: Some(0),
                duration_ms: None,
            },
        },
    ));

    assert_eq!(status, CodexStatus::InitiateShutdown);
    assert_eq!(processor.final_message(), None);

    EventProcessor::print_final_output(&mut processor);

    assert_eq!(
        std::fs::read_to_string(&output_path).expect("read output file"),
        "keep existing contents"
    );
}

#[test]
fn runtime_warning_emits_a_non_fatal_error_item() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);

    let collected = processor.collect_thread_events(ServerNotification::Warning(
        codex_app_server_protocol::WarningNotification {
            thread_id: Some("thread-1".to_string()),
            message: "invalid global instructions".to_string(),
        },
    ));

    assert_eq!(
        collected,
        CollectedThreadEvents {
            events: vec![ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ExecThreadItem {
                    id: "item_0".to_string(),
                    details: ThreadItemDetails::Error(ErrorItem {
                        message: "invalid global instructions".to_string(),
                    }),
                },
            })],
            status: CodexStatus::Running,
        }
    );
}

#[test]
fn request_usage_falls_back_only_for_reconciled_new_token_counts() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);
    let last = TokenUsageBreakdown {
        input_tokens: 120,
        cached_input_tokens: 100,
        cache_write_input_tokens: 5,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 150,
    };
    let notification =
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: ThreadTokenUsage {
                total: last.clone(),
                last: last.clone(),
                model_context_window: None,
            },
        });
    let expected = CollectedThreadEvents {
        status: CodexStatus::Running,
        events: vec![ThreadEvent::ModelUsage(ModelUsageEvent {
            response_id: None,
            request_index: Some(1),
            usage_source: "token_usage_last_reconciled".to_string(),
            usage: Usage {
                input_tokens: 120,
                cached_input_tokens: 100,
                cache_write_input_tokens: 5,
                output_tokens: 30,
                reasoning_output_tokens: 10,
            },
        })],
    };
    assert_eq!(
        processor.collect_thread_events(notification.clone()),
        expected
    );
    assert_eq!(
        processor.collect_thread_events(notification),
        CollectedThreadEvents {
            events: Vec::new(),
            status: CodexStatus::Running
        }
    );

    let inconsistent =
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: ThreadTokenUsage {
                total: TokenUsageBreakdown {
                    input_tokens: 999,
                    ..last.clone()
                },
                last,
                model_context_window: None,
            },
        });
    assert_eq!(
        processor.collect_thread_events(inconsistent),
        CollectedThreadEvents {
            events: Vec::new(),
            status: CodexStatus::Running
        }
    );
}

#[test]
fn direct_response_usage_is_not_repeated_by_token_count_fallback() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);
    let usage = TokenUsageBreakdown {
        input_tokens: 120,
        cached_input_tokens: 100,
        cache_write_input_tokens: 5,
        output_tokens: 30,
        reasoning_output_tokens: 10,
        total_tokens: 150,
    };
    let direct = processor.collect_thread_events(ServerNotification::RawResponseCompleted(
        RawResponseCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            response_id: "resp-1".to_string(),
            usage: Some(usage.clone()),
            usage_metadata: None,
        },
    ));
    assert_eq!(direct.events.len(), 1);
    let counted = processor.collect_thread_events(ServerNotification::ThreadTokenUsageUpdated(
        ThreadTokenUsageUpdatedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: ThreadTokenUsage {
                total: usage.clone(),
                last: usage,
                model_context_window: None,
            },
        },
    ));
    assert_eq!(
        counted,
        CollectedThreadEvents {
            events: Vec::new(),
            status: CodexStatus::Running
        }
    );
}

#[test]
fn raw_response_completion_emits_exact_request_usage() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);

    let collected = processor.collect_thread_events(ServerNotification::RawResponseCompleted(
        RawResponseCompletedNotification {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            response_id: "resp-1".to_string(),
            usage: Some(TokenUsageBreakdown {
                total_tokens: 150,
                input_tokens: 120,
                cached_input_tokens: 100,
                cache_write_input_tokens: 5,
                output_tokens: 30,
                reasoning_output_tokens: 10,
            }),
            usage_metadata: None,
        },
    ));

    assert_eq!(collected.status, CodexStatus::Running);
    assert_eq!(
        collected.events,
        vec![ThreadEvent::ModelUsage(ModelUsageEvent {
            response_id: Some("resp-1".to_string()),
            request_index: None,
            usage_source: "raw_response_completed".to_string(),
            usage: Usage {
                input_tokens: 120,
                cached_input_tokens: 100,
                cache_write_input_tokens: 5,
                output_tokens: 30,
                reasoning_output_tokens: 10,
            },
        })]
    );
}

#[test]
fn mcp_tool_call_result_preserves_meta_in_jsonl_event() {
    let mut processor = EventProcessorWithJsonOutput::new(/*last_message_path*/ None);

    let collected = processor.collect_thread_events(ServerNotification::ItemCompleted(
        codex_app_server_protocol::ItemCompletedNotification {
            item: ThreadItem::McpToolCall {
                id: "mcp-1".to_string(),
                server: "search service".to_string(),
                tool: "web_run".to_string(),
                status: McpToolCallStatus::Completed,
                arguments: json!({"search_query": [{"q": "OpenAI Codex CLI documentation"}]}),
                app_context: None,
                mcp_app_resource_uri: None,
                mcp_app_ui: None,
                plugin_id: None,
                read_only_hint: None,
                result: Some(Box::new(codex_app_server_protocol::McpToolCallResult {
                    content: vec![json!({"type": "text", "text": "search result"})],
                    structured_content: None,
                    meta: Some(json!({"raw_messages": [{"ref_id": "turn0search0"}]})),
                })),
                error: None,
                duration_ms: Some(42),
            },
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            completed_at_ms: 0,
        },
    ));

    assert_eq!(collected.status, CodexStatus::Running);
    assert_eq!(collected.events.len(), 1);

    let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = &collected.events[0] else {
        panic!("expected item.completed event");
    };
    let ThreadItemDetails::McpToolCall(item) = &item.details else {
        panic!("expected MCP tool call item");
    };
    let result = item.result.as_ref().expect("expected MCP tool result");
    assert_eq!(
        result.meta,
        Some(json!({"raw_messages": [{"ref_id": "turn0search0"}]}))
    );

    let serialized = serde_json::to_value(&collected.events[0]).expect("serialize event");
    assert_eq!(
        serialized["item"]["result"]["_meta"],
        json!({"raw_messages": [{"ref_id": "turn0search0"}]})
    );
    assert!(serialized["item"]["result"].get("meta").is_none());
}
