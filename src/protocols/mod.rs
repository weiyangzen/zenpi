//! Shared provider wire encoding and bounded response decoding.
pub(crate) mod anthropic;
pub(crate) mod chat;
pub(crate) mod content;
pub(crate) mod google;
mod responses;

#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::auth::{AllowedDestination, AuthBinding, AuthIdentitySnapshot};
    use crate::core::Turn;
    use crate::providers::connection::{ProviderConnection, resolve_model_route};
    use crate::providers::registry::{FieldSource, ModelRegistry, ReasoningLevel};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn route(provider: &str, protocol: Protocol, streaming: bool) -> ValidatedRoute {
        route_with_schema(provider, protocol, streaming, true)
    }

    fn route_with_schema(
        provider: &str,
        protocol: Protocol,
        streaming: bool,
        schema: bool,
    ) -> ValidatedRoute {
        let definition = crate::providers::get_provider_definition(provider).unwrap();
        let mut model = ModelRegistry::default()
            .resolve(provider, "fixture-model")
            .unwrap();
        model.capabilities = ProviderCapabilities {
            text: true,
            images: true,
            files: true,
            tools: true,
            structured_output: schema,
            streaming: true,
            reasoning: true,
        };
        model.reasoning_levels = [
            ReasoningLevel::None,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
            ReasoningLevel::Max,
        ]
        .into();
        for field in [
            "images",
            "files",
            "tools",
            "structured_output",
            "reasoning_levels",
        ] {
            model.sources.insert(
                field.into(),
                FieldSource::UserOverride {
                    version: "fixture".into(),
                },
            );
        }
        let codex = protocol == Protocol::OpenAiCodexResponses;
        let connection = ProviderConnection {
            profile: "fixture".into(),
            provider: provider.into(),
            protocol,
            base_url: None,
            auth: if codex {
                AuthBinding::CodexOAuth {
                    credential_id: "fixture".into(),
                }
            } else {
                AuthBinding::StoredApiKey {
                    credential_id: "fixture".into(),
                }
            },
            header_policy: None,
            config_revision: 1,
            model_routes: vec![],
        };
        let identity = AuthIdentitySnapshot {
            provider: provider.into(),
            credential_id: "fixture".into(),
            account_id: Some("fixture-account".into()),
            identity_generation: "fixture-generation".into(),
            credential_revision: 1,
            allowed_destinations: vec![AllowedDestination {
                origin: match provider {
                    "openai-codex" => "https://chatgpt.com",
                    "openai" => "https://api.openai.com",
                    "anthropic" => "https://api.anthropic.com",
                    "google" => "https://generativelanguage.googleapis.com",
                    _ => "https://api.deepseek.com",
                }
                .into(),
                path_prefix: "/".into(),
                protocols: vec![protocol.as_str().into()],
                headers: vec![
                    "authorization".into(),
                    "chatgpt-account-id".into(),
                    "x-api-key".into(),
                    "x-goog-api-key".into(),
                ],
            }],
        };
        resolve_model_route(definition, &connection, &model, Some(&identity), streaming).unwrap()
    }

    fn options(route: &ValidatedRoute) -> RequestOptions<'_> {
        RequestOptions {
            wire_api: route.protocol().wire_api(),
            model: &route.model().id,
            provider: Some(route.provider()),
            reasoning_effort: None,
            verbosity: None,
            capabilities: route.capabilities(),
            descriptor: Some(route.model()),
        }
    }

    fn tool_history(id: &str) -> Vec<Turn> {
        let mut assistant = Turn::new("assistant", TurnRole::Assistant, "checking");
        assistant.metadata =
            Some(json!({"tool_calls":[{"id":id,"name":"lookup","arguments":{"value":1}}]}));
        let mut tool = Turn::new("tool", TurnRole::Tool, "result");
        tool.metadata = Some(json!({"tool_call_id":id,"outcome":"succeeded"}));
        vec![
            Turn::new("user", TurnRole::User, "question"),
            assistant,
            tool,
        ]
    }

    #[test]
    fn codex_body_preserves_full_history_and_never_silently_drops_a_hard_cap() {
        let route = route("openai-codex", Protocol::OpenAiCodexResponses, true);
        let options = options(&route);
        let turns = tool_history("call_ok-1");
        let metadata = json!({"store":true,"stream":false,"max_output_tokens":7});
        let request = CompletionRequest::new("op", &turns, Some("fixture-model"), &[])
            .with_instructions(Some("host instructions"))
            .with_metadata(Some(&metadata));
        let body = encode_request_for_route(&request, &options, &route).unwrap();
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "host instructions");
        assert_eq!(body["input"].as_array().unwrap().len(), 4);
        assert_eq!(body["input"][1]["content"], "checking");
        assert_eq!(body["input"][2]["call_id"], "call_ok-1");
        assert_eq!(body["input"][3]["call_id"], "call_ok-1");
        assert!(body.get("max_output_tokens").is_none());
        let request = request.with_max_output_tokens(7);
        assert!(encode_request_for_route(&request, &options, &route).is_err());
    }

    #[test]
    fn codex_call_mapping_is_stable_paired_and_detects_collision_and_orphans() {
        let route = route("openai-codex", Protocol::OpenAiCodexResponses, true);
        let options = options(&route);
        let id = "provider/session:call with spaces";
        let mut turns = tool_history(id);
        let encode = |turns: &[Turn]| {
            encode_request_for_route(
                &CompletionRequest::new("op", turns, None, &[]),
                &options,
                &route,
            )
        };
        let body = encode(&turns).unwrap();
        let mapped = body["input"][2]["call_id"].as_str().unwrap().to_owned();
        assert!(mapped.starts_with("zpi1_") && mapped.len() == 64);
        assert_eq!(body["input"][3]["call_id"], mapped);
        assert_eq!(
            turns[1].metadata.as_ref().unwrap()["tool_calls"][0]["id"],
            id
        );
        assert_eq!(encode(&turns).unwrap(), body);
        turns.extend(tool_history(&mapped));
        assert!(encode(&turns).is_err());
        assert!(encode(&tool_history("valid")[2..]).is_err());
        assert!(encode(&tool_history("valid")[..2]).is_err());
        let mut duplicate = tool_history("valid");
        duplicate.push(duplicate[2].clone());
        assert!(encode(&duplicate).is_err());
    }

    #[test]
    fn explicit_route_rejects_mismatched_options_and_unscoped_codex_history() {
        let route = route("openai-codex", Protocol::OpenAiCodexResponses, true);
        let mut options = options(&route);
        let mut turns = vec![Turn::new("user", TurnRole::User, "question")];
        options.provider = Some("openai");
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[]),
                &options,
                &route
            )
            .is_err()
        );
        options.provider = Some(route.provider());
        options.capabilities.images = !options.capabilities.images;
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[]),
                &options,
                &route
            )
            .is_err()
        );
        options.capabilities = route.capabilities();
        options.descriptor = None;
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[]),
                &options,
                &route
            )
            .is_err()
        );
        options.descriptor = Some(route.model());
        turns[0].metadata = Some(
            json!({"annotations":[{"type":"native_history","wire":"responses","encrypted_content":"opaque"}]}),
        );
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[]),
                &options,
                &route
            )
            .is_err()
        );
    }

    #[test]
    fn deepseek_routes_have_distinct_limits_reasoning_and_json_modes() {
        let turns = [Turn::new("user", TurnRole::User, "Return JSON.")];
        let json_mode = json!({"type":"json_object"});
        let schema = json!({"type":"json_schema","json_schema":{"name":"answer","schema":{"type":"object"}}});
        for protocol in [
            Protocol::ChatCompletions,
            Protocol::Responses,
            Protocol::AnthropicMessages,
        ] {
            let route = route("deepseek", protocol, true);
            let mut options = options(&route);
            options.reasoning_effort = Some("medium");
            let request =
                CompletionRequest::new("op", &turns, None, &[]).with_max_output_tokens(128);
            let body = encode_request_for_route(&request, &options, &route).unwrap();
            assert!(body.get("max_completion_tokens").is_none());
            match protocol {
                Protocol::ChatCompletions => {
                    assert_eq!(body["max_tokens"], 128);
                    assert_eq!(body["reasoning_effort"], "high");
                    assert_eq!(body["thinking"]["type"], "enabled");
                    let legacy = encode_request(&request, &options).unwrap();
                    assert_eq!(legacy["max_completion_tokens"], 128);
                    assert!(legacy.get("thinking").is_none());
                }
                Protocol::Responses => {
                    assert_eq!(body["max_output_tokens"], 128);
                    assert_eq!(body["reasoning"]["effort"], "high");
                    assert!(body.get("store").is_none());
                }
                _ => {
                    assert_eq!(body["max_tokens"], 128);
                    assert_eq!(body["thinking"], json!({"type":"enabled"}));
                    assert_eq!(body["output_config"], json!({"effort":"high"}));
                }
            }
            let json_request = CompletionRequest::new("op", &turns, None, &[])
                .with_response_format(Some(&json_mode));
            assert_eq!(
                encode_request_for_route(&json_request, &options, &route).is_ok(),
                protocol != Protocol::AnthropicMessages
            );
            let schema_request =
                CompletionRequest::new("op", &turns, None, &[]).with_response_format(Some(&schema));
            assert_eq!(
                encode_request_for_route(&schema_request, &options, &route).is_ok(),
                protocol == Protocol::Responses
            );
            options.verbosity = Some("low");
            assert!(encode_request_for_route(&request, &options, &route).is_err());
        }
    }

    #[test]
    fn deepseek_chat_replays_reasoning_content_for_tool_continuation() {
        let route = route("deepseek", Protocol::ChatCompletions, false);
        let options = options(&route);
        let payload = json!({"model":"fixture-model","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"reasoning_content":"private reasoning","tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{}"}}]}}]});
        let completion = read_response_for_route(
            &mut ureq::Body::builder().data(payload.to_string()),
            false,
            &options,
            &route,
            &|| false,
            &mut |_| Ok(()),
        )
        .unwrap();
        let mut turns = tool_history("call_1");
        turns[1].content = completion.content;
        turns[1].metadata =
            Some(json!({"tool_calls":completion.tool_calls,"annotations":completion.annotations}));
        let body = encode_request_for_route(
            &CompletionRequest::new("op", &turns, None, &[]),
            &options,
            &route,
        )
        .unwrap();
        assert_eq!(
            body["messages"][1]["reasoning_content"],
            "private reasoning"
        );
        assert_ne!(body["messages"][1]["content"], "private reasoning");
        let mut opaque = payload;
        opaque["choices"][0]["message"]["reasoning_details"] =
            json!([{"type":"reasoning.encrypted","data":"opaque"}]);
        let mut completed = false;
        assert!(
            read_response_for_route(
                &mut ureq::Body::builder().data(opaque.to_string()),
                false,
                &options,
                &route,
                &|| false,
                &mut |event| {
                    completed |= matches!(event, ProviderEvent::Completed { .. });
                    Ok(())
                }
            )
            .is_err()
        );
        assert!(!completed);
        let legacy = read_response(
            &mut ureq::Body::builder().data(opaque.to_string()),
            false,
            &options,
            &|| false,
            &mut |_| Ok(()),
        )
        .unwrap();
        turns[1].metadata =
            Some(json!({"tool_calls":legacy.tool_calls,"annotations":legacy.annotations}));
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[]),
                &options,
                &route
            )
            .is_err()
        );
    }

    #[test]
    fn deepseek_responses_json_mode_does_not_claim_json_schema_capability() {
        let route = route_with_schema("deepseek", Protocol::Responses, false, false);
        let options = options(&route);
        assert!(!options.capabilities.structured_output);
        let turns = [Turn::new("user", TurnRole::User, "Return JSON.")];
        for format in [json!({"type":"text"}), json!({"type":"json_object"})] {
            let request =
                CompletionRequest::new("op", &turns, None, &[]).with_response_format(Some(&format));
            let body = encode_request_for_route(&request, &options, &route).unwrap();
            assert_eq!(body["text"]["format"], format);
        }
        let schema = json!({"type":"json_schema","json_schema":{"name":"answer","schema":{"type":"object"}}});
        assert!(
            encode_request_for_route(
                &CompletionRequest::new("op", &turns, None, &[])
                    .with_response_format(Some(&schema)),
                &options,
                &route
            )
            .is_err()
        );
    }

    #[test]
    fn all_explicit_responses_routes_reject_unscoped_opaque_history_before_encoding() {
        for (provider, protocol) in [
            ("openai-codex", Protocol::OpenAiCodexResponses),
            ("deepseek", Protocol::Responses),
        ] {
            let route = route(provider, protocol, true);
            let options = options(&route);
            for metadata in [
                json!({"encrypted_content":"opaque"}),
                json!({"annotations":[{"type":"native_history","wire":"responses","encrypted_content":"opaque"}]}),
            ] {
                let mut assistant = Turn::new("assistant", TurnRole::Assistant, "visible");
                assistant.metadata = Some(metadata);
                let turns = [assistant, Turn::new("user", TurnRole::User, "continue")];
                assert!(
                    encode_request_for_route(
                        &CompletionRequest::new("op", &turns, None, &[]),
                        &options,
                        &route
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn default_anthropic_accepts_first_result_but_rejects_unscoped_signed_replay() {
        let route = route("anthropic", Protocol::AnthropicMessages, false);
        for opaque in [
            None,
            Some(json!({"type":"thinking","thinking":"private","signature":"synthetic-signature"})),
            Some(json!({"type":"redacted_thinking","data":"synthetic-opaque"})),
        ] {
            let mut blocks = vec![json!({"type":"text","text":"visible"})];
            if let Some(block) = &opaque {
                blocks.push(block.clone());
            }
            let payload = json!({"type":"message","id":"msg_1","role":"assistant","model":"fixture-model","stop_reason":"end_turn","content":blocks,"usage":{"input_tokens":1,"output_tokens":1}});
            assert_replay_scope_guard(&route, payload, opaque.is_some());
        }
    }

    #[test]
    fn default_google_accepts_first_result_but_rejects_unscoped_thought_replay() {
        let route = route("google", Protocol::GoogleGenerativeAi, false);
        for opaque in [
            None,
            Some(json!({"text":"signed","thoughtSignature":"c2ln"})),
            Some(json!({"text":"private","thought":true})),
        ] {
            let mut parts = vec![json!({"text":"visible"})];
            if let Some(part) = &opaque {
                parts.push(part.clone());
            }
            let payload = json!({"responseId":"response_1","modelVersion":"fixture-model","candidates":[{"index":0,"content":{"role":"model","parts":parts},"finishReason":"STOP"}]});
            assert_replay_scope_guard(&route, payload, opaque.is_some());
        }
    }

    #[test]
    fn default_chat_rejects_encrypted_replay_without_rejecting_plain_reasoning() {
        let route = route("openai", Protocol::ChatCompletions, false);
        for detail in [
            None,
            Some(json!({"type":"reasoning.text","text":"plain detail"})),
            Some(json!({"type":"reasoning.encrypted","data":"synthetic-opaque"})),
        ] {
            let encrypted = detail
                .as_ref()
                .is_some_and(|detail| detail["type"] == "reasoning.encrypted");
            let mut message = json!({"role":"assistant","content":"visible","reasoning_content":"plain reasoning"});
            if let Some(detail) = detail {
                message["reasoning_details"] = json!([detail]);
            }
            let payload = json!({"model":"fixture-model","choices":[{"index":0,"finish_reason":"stop","message":message}]});
            assert_replay_scope_guard(&route, payload, encrypted);
        }
    }

    fn assert_replay_scope_guard(route: &ValidatedRoute, payload: Value, reject: bool) {
        let options = options(route);
        let completion = read_response_for_route(
            &mut ureq::Body::builder().data(payload.to_string()),
            false,
            &options,
            route,
            &|| false,
            &mut |_| Ok(()),
        )
        .unwrap();
        let mut assistant = Turn::new("assistant", TurnRole::Assistant, completion.content);
        assistant.metadata = Some(json!({"annotations":completion.annotations}));
        let turns = [
            Turn::new("user", TurnRole::User, "question"),
            assistant,
            Turn::new("followup", TurnRole::User, "continue"),
        ];
        let request = CompletionRequest::new("op", &turns, None, &[]);
        assert!(encode_request(&request, &options).is_ok());
        let result = encode_request_for_route(&request, &options, route);
        if reject {
            assert!(
                matches!(result, Err(BackendError::Configuration(message)) if message == "opaque history requires a verified identity scope")
            );
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn deepseek_messages_preserves_unsigned_thinking_but_rejects_ignored_error_semantics() {
        let route = route("deepseek", Protocol::AnthropicMessages, false);
        let options = options(&route);
        let payload = json!({"type":"message","id":"msg_1","role":"assistant","model":"actual-serving-model","stop_reason":"tool_use","content":[{"type":"thinking","thinking":"private reasoning"},{"type":"tool_use","id":"call_1","name":"lookup","input":{}}],"usage":{"input_tokens":2,"output_tokens":3}});
        let completion = read_response_for_route(
            &mut ureq::Body::builder().data(payload.to_string()),
            false,
            &options,
            &route,
            &|| false,
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!(completion.model.as_deref(), Some("actual-serving-model"));
        assert!(completion.content.is_empty());
        let mut turns = tool_history("call_1");
        turns[1].content = completion.content;
        turns[1].metadata =
            Some(json!({"tool_calls":completion.tool_calls,"annotations":completion.annotations}));
        let encode = |turns: &[Turn]| {
            encode_request_for_route(
                &CompletionRequest::new("op", turns, None, &[]),
                &options,
                &route,
            )
        };
        let body = encode(&turns).unwrap();
        assert_eq!(
            body["messages"][1]["content"][0],
            json!({"type":"thinking","thinking":"private reasoning"})
        );
        assert!(body["messages"][2]["content"][0].get("is_error").is_none());
        turns[2].metadata.as_mut().unwrap()["outcome"] = json!("failed");
        assert!(encode(&turns).is_err());
    }

    fn completed(text: &str) -> String {
        format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","model":"fixture-model","output":[{"type":"message","content":[{"type":"output_text","text":text}]}]}})
        )
    }

    #[test]
    fn routed_responses_share_bounded_multiline_utf8_reducer_and_strict_terminals() {
        struct Bytes(std::io::Cursor<Vec<u8>>);
        impl std::io::Read for Bytes {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let length = buffer.len().min(1);
                std::io::Read::read(&mut self.0, &mut buffer[..length])
            }
        }
        let route = route("openai-codex", Protocol::OpenAiCodexResponses, true);
        let options = options(&route);
        let stream = format!(
            "\0event: response.output_text.delta\r\ndata: {{\"type\":\"response.output_text.delta\",\r\ndata: \"delta\":\"caf\u{e9}\"}}\r\n\r\n{}",
            completed("caf\u{e9}")
        );
        let mut events = Vec::new();
        let completion = read_response_for_route(
            &mut ureq::Body::builder().reader(Bytes(std::io::Cursor::new(stream.into_bytes()))),
            true,
            &options,
            &route,
            &|| false,
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(completion.content, "caf\u{e9}");
        assert!(matches!(
            events.first(),
            Some(ProviderEvent::TextDelta { .. })
        ));
        for stream in [
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n".to_owned(),
            "data: {\"type\":\"response.incomplete\"}\n\n".into(),
            "data: {\"type\":\"response.failed\"}\n\n".into(),
            format!("{}{}", completed("x"), completed("x")),
            format!(
                "{}data: {{\"type\":\"response.incomplete\"}}\n\n",
                completed("x")
            ),
            "data: [DONE]\n\n".into(),
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}".into(),
            format!("data: {}\n\n", "x".repeat(MAX_RESPONSE_BYTES + 1)),
        ] {
            assert!(
                read_response_for_route(
                    &mut ureq::Body::builder().data(stream),
                    true,
                    &options,
                    &route,
                    &|| false,
                    &mut |_| Ok(())
                )
                .is_err()
            );
        }
    }

    #[test]
    fn routed_responses_cancel_between_events_and_never_flatten_reasoning() {
        let route = route("deepseek", Protocol::Responses, true);
        let options = options(&route);
        let cancelled = AtomicBool::new(false);
        let mut count = 0;
        let stream = format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"x\"}}\n\n{}",
            completed("x")
        );
        let result = read_response_for_route(
            &mut ureq::Body::builder().data(stream),
            true,
            &options,
            &route,
            &|| cancelled.load(Ordering::SeqCst),
            &mut |_| {
                count += 1;
                cancelled.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(matches!(result, Err(BackendError::Cancelled)));
        assert_eq!(count, 1);
        for encrypted in [false, true] {
            let mut reasoning = json!({"type":"reasoning","content":[{"type":"reasoning_text","text":"not the answer"}]});
            if encrypted {
                reasoning["encrypted_content"] = json!("opaque");
            }
            let stream = format!(
                "data: {}\n\n",
                json!({"type":"response.completed","response":{"status":"completed","output":[reasoning,{"type":"message","content":[{"type":"output_text","text":"answer"}]}]}})
            );
            let result = read_response_for_route(
                &mut ureq::Body::builder().data(stream),
                true,
                &options,
                &route,
                &|| false,
                &mut |_| Ok(()),
            );
            if encrypted {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap().content, "answer");
            }
        }
    }

    #[test]
    fn routed_responses_tool_calls_require_complete_consistent_terminal_arguments() {
        let route = route("openai-codex", Protocol::OpenAiCodexResponses, true);
        let options = options(&route);
        let item = json!({"type":"function_call","id":"fc_item","call_id":"call_1","name":"lookup","arguments":"{}","status":"completed"});
        let terminal = |item: Value| json!({"type":"response.completed","response":{"status":"completed","output":[item]}});
        let stream = [
            json!({"type":"response.function_call_arguments.delta","item_id":"fc_item","delta":"{}"}),
            json!({"type":"response.function_call_arguments.done","item_id":"fc_item","arguments":"{}"}),
            json!({"type":"response.output_item.done","item":item}),
            terminal(item.clone()),
        ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
        let mut ready = 0;
        let completion = read_response_for_route(
            &mut ureq::Body::builder().data(stream),
            true,
            &options,
            &route,
            &|| false,
            &mut |event| {
                if matches!(event, ProviderEvent::ToolCallDone { .. }) {
                    ready += 1;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(ready, 1);
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].arguments, json!({}));
        for malformed in [Value::Null, json!("{"), json!("[]")] {
            let mut bad_item = item.clone();
            bad_item["arguments"] = malformed;
            let stream = format!("data: {}\n\n", terminal(bad_item));
            assert!(
                read_response_for_route(
                    &mut ureq::Body::builder().data(stream),
                    true,
                    &options,
                    &route,
                    &|| false,
                    &mut |_| Ok(())
                )
                .is_err()
            );
        }
        let mut conflicting = item.clone();
        conflicting["arguments"] = json!("{\"changed\":true}");
        let stream = format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"type":"response.output_item.done","item":item}),
            terminal(conflicting)
        );
        assert!(
            read_response_for_route(
                &mut ureq::Body::builder().data(stream),
                true,
                &options,
                &route,
                &|| false,
                &mut |_| Ok(())
            )
            .is_err()
        );
    }
}

use crate::backend::{
    AttachmentKind, BackendError, Completion, CompletionRequest, MAX_RESPONSE_BYTES, OpenAiWireApi,
    ProviderCapabilities, ProviderEvent, Usage,
};
use crate::core::TurnRole;
use crate::providers::registry::ModelDescriptor;
use crate::providers::{Dialect, Protocol, connection::ValidatedRoute};
use crate::tools::ToolCall;
use serde_json::{Value, json};

pub(crate) struct RequestOptions<'a> {
    pub wire_api: OpenAiWireApi,
    pub model: &'a str,
    pub provider: Option<&'a str>,
    pub reasoning_effort: Option<&'a str>,
    pub verbosity: Option<&'a str>,
    pub capabilities: ProviderCapabilities,
    pub descriptor: Option<&'a ModelDescriptor>,
}

impl RequestOptions<'_> {
    fn provider(&self) -> &str {
        self.provider.unwrap_or(match self.wire_api {
            OpenAiWireApi::AnthropicMessages => "anthropic",
            OpenAiWireApi::GoogleGenerativeAi => "google",
            _ => "openai",
        })
    }
}

pub(crate) fn encode_request(
    request: &CompletionRequest<'_>,
    options: &RequestOptions<'_>,
) -> Result<Value, BackendError> {
    encode_request_inner(request, options, None)
}

pub(crate) fn encode_request_for_route(
    request: &CompletionRequest<'_>,
    options: &RequestOptions<'_>,
    route: &ValidatedRoute,
) -> Result<Value, BackendError> {
    validate_route_options(options, route)?;
    if request.model.is_some_and(|model| model != route.model().id) {
        return Err(configuration(
            "request model does not match validated route",
        ));
    }
    if request.max_output_tokens.is_some() && !route.options().output_token_limit {
        return Err(configuration(
            "route does not support a remote output token limit",
        ));
    }
    if options.reasoning_effort.is_some() && !route.options().reasoning_effort {
        return Err(configuration("route does not support reasoning effort"));
    }
    route
        .model()
        .validate_reasoning(route.capabilities(), options.reasoning_effort)
        .map_err(|_| configuration("model does not support this reasoning effort"))?;
    if !request.tools.is_empty() && !route.options().tool_choice {
        return Err(configuration("route does not support function tools"));
    }
    if request.response_format.is_some() && !route.options().response_format {
        return Err(configuration("route does not support this response format"));
    }
    if route.dialect() == Dialect::Codex
        && (!route.streaming() || route.protocol() != Protocol::OpenAiCodexResponses)
    {
        return Err(configuration("Codex requires streamed Responses"));
    }
    if options.wire_api == OpenAiWireApi::Responses {
        for turn in request.turns {
            if turn.metadata.as_ref().is_some_and(|metadata| {
                metadata.get("encrypted_content").is_some()
                    || metadata
                        .get("annotations")
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item["type"] == "native_history"
                                    || item.get("encrypted_content").is_some()
                            })
                        })
            }) {
                return Err(configuration(
                    "Responses opaque history requires a verified identity scope",
                ));
            }
        }
    }
    if route.dialect() == Dialect::Default
        && request.turns.iter().any(|turn| {
            turn.metadata
                .as_ref()
                .and_then(|metadata| metadata["annotations"].as_array())
                .is_some_and(|annotations| {
                    annotations.iter().any(|annotation| {
                        if annotation["type"] != "native_history" {
                            return false;
                        }
                        match options.wire_api {
                            OpenAiWireApi::AnthropicMessages => {
                                annotation["blocks"].as_array().is_some_and(|blocks| {
                                    blocks.iter().any(|block| {
                                        matches!(
                                            block["type"].as_str(),
                                            Some("thinking" | "redacted_thinking")
                                        )
                                    })
                                })
                            }
                            OpenAiWireApi::GoogleGenerativeAi => {
                                annotation["parts"].as_array().is_some_and(|parts| {
                                    parts.iter().any(|part| {
                                        part.get("thoughtSignature").is_some()
                                            || part["thought"] == true
                                    })
                                })
                            }
                            OpenAiWireApi::ChatCompletions => {
                                annotation["chat_reasoning"]["details"]
                                    .as_array()
                                    .is_some_and(|details| {
                                        details
                                            .iter()
                                            .any(|detail| detail["type"] == "reasoning.encrypted")
                                    })
                            }
                            OpenAiWireApi::Responses => false,
                        }
                    })
                })
        })
    {
        return Err(configuration(
            "opaque history requires a verified identity scope",
        ));
    }
    if route.dialect() == Dialect::DeepSeek {
        if options.verbosity.is_some() {
            return Err(configuration("DeepSeek does not enforce verbosity"));
        }
        if let Some(effort) = options.reasoning_effort {
            deepseek_effort(effort)?;
        }
        if route.protocol() == Protocol::ChatCompletions
            && request.turns.iter().any(|turn| {
                turn.metadata
                    .as_ref()
                    .and_then(|metadata| metadata["annotations"].as_array())
                    .is_some_and(|annotations| unsupported_deepseek_chat_details(annotations))
            })
        {
            return Err(configuration(
                "DeepSeek Chat does not support opaque reasoning details",
            ));
        }
        if route.protocol() != Protocol::AnthropicMessages
            && request.metadata.is_some_and(|metadata| {
                metadata.as_object().is_none_or(|object| {
                    object
                        .iter()
                        .any(|(key, value)| key != "purpose" || value != "semantic_compaction")
                })
            })
        {
            return Err(configuration(
                "DeepSeek route does not support provider metadata",
            ));
        }
        if route.protocol() == Protocol::ChatCompletions
            && request
                .response_format
                .is_some_and(|format| format["type"] == "json_schema")
        {
            return Err(configuration(
                "DeepSeek Chat supports JSON mode, not JSON Schema",
            ));
        }
    }
    encode_request_inner(request, options, Some(route))
}

fn configuration(message: &'static str) -> BackendError {
    BackendError::Configuration(message.into())
}

fn unsupported_deepseek_chat_details(annotations: &[Value]) -> bool {
    annotations.iter().any(|annotation| {
        annotation["chat_reasoning"]
            .get("details")
            .is_some_and(|details| details.as_array().is_none_or(|details| !details.is_empty()))
    })
}

fn validate_route_options(
    options: &RequestOptions<'_>,
    route: &ValidatedRoute,
) -> Result<(), BackendError> {
    if options.wire_api != route.protocol().wire_api()
        || options.model != route.model().id
        || options.provider != Some(route.provider())
        || options.capabilities != route.capabilities()
        || options.descriptor != Some(route.model())
    {
        return Err(configuration(
            "protocol options do not match validated route",
        ));
    }
    Ok(())
}

fn deepseek_effort(effort: &str) -> Result<&str, BackendError> {
    match effort {
        "none" | "low" | "high" | "max" => Ok(effort),
        "minimal" => Ok("low"),
        "medium" | "xhigh" => Ok("high"),
        _ => Err(configuration("unsupported DeepSeek reasoning effort")),
    }
}

fn encode_request_inner(
    request: &CompletionRequest<'_>,
    options: &RequestOptions<'_>,
    route: Option<&ValidatedRoute>,
) -> Result<Value, BackendError> {
    let wire_api = options.wire_api;
    let model = options.model;
    let provider = options.provider();
    let capabilities = options.capabilities;
    let descriptor = options.descriptor;
    let reasoning_effort = options.reasoning_effort;
    let verbosity = options.verbosity;
    if !capabilities.text {
        return Err(BackendError::Configuration(
            "selected model does not support text".into(),
        ));
    }
    if !capabilities.tools
        && (!request.tools.is_empty()
            || request.turns.iter().any(|turn| {
                turn.role == TurnRole::Tool
                    || turn
                        .metadata
                        .as_ref()
                        .is_some_and(|metadata| metadata.get("tool_calls").is_some())
            }))
    {
        return Err(BackendError::Configuration(
            "selected model does not support tools".into(),
        ));
    }
    for attachment in request.attachments {
        if (attachment.input.kind == AttachmentKind::Image && !capabilities.images)
            || (attachment.input.kind == AttachmentKind::File && !capabilities.files)
        {
            return Err(BackendError::Configuration(
                "selected model or wire does not support attachment kind".into(),
            ));
        }
    }
    if let Some(format) = request.response_format {
        let json_mode = route.is_some_and(|route| {
            route.dialect() == Dialect::DeepSeek
                && matches!(
                    route.protocol(),
                    Protocol::ChatCompletions | Protocol::Responses
                )
                && matches!(format["type"].as_str(), Some("text" | "json_object"))
        });
        if !capabilities.structured_output && !json_mode {
            return Err(BackendError::Configuration(
                "selected model does not support structured output".into(),
            ));
        }
        if !(json_mode && format == &json!({"type":"text"})) {
            validate_response_format(format)?;
        }
    }
    anthropic::validate_history_wire(request.turns, wire_api.as_str())?;
    if matches!(
        wire_api,
        OpenAiWireApi::AnthropicMessages | OpenAiWireApi::GoogleGenerativeAi
    ) && verbosity.is_some()
    {
        return Err(BackendError::Configuration(
            "Native provider verbosity is unsupported".into(),
        ));
    }
    let mut body = match wire_api {
        OpenAiWireApi::GoogleGenerativeAi => {
            google::request_body(request, model, reasoning_effort, provider)?
        }
        OpenAiWireApi::AnthropicMessages => {
            if route.is_some_and(|route| route.dialect() == Dialect::DeepSeek) {
                anthropic::request_body_deepseek(request, model, reasoning_effort, provider)?
            } else {
                anthropic::request_body(request, model, reasoning_effort, provider)?
            }
        }
        OpenAiWireApi::ChatCompletions => chat::encode_request(request, model, provider)?,
        OpenAiWireApi::Responses => {
            if let Some(route) = route {
                responses::encode_request_for_route(
                    request,
                    model,
                    reasoning_effort,
                    verbosity,
                    route.dialect() == Dialect::Codex,
                )?
            } else {
                responses::encode_request(request, model, reasoning_effort, verbosity)?
            }
        }
    };
    if wire_api != OpenAiWireApi::GoogleGenerativeAi {
        body["stream"] = json!(capabilities.streaming);
    }
    if !capabilities.streaming {
        body.as_object_mut()
            .expect("request object")
            .remove("stream_options");
    }
    // Anthropic/Google builders default the field themselves; OpenAI wires
    // send a limit only when the caller explicitly requested one, because
    // some Responses upstreams reject the field outright.
    let openai_wire = matches!(
        wire_api,
        OpenAiWireApi::Responses | OpenAiWireApi::ChatCompletions
    );
    let limit = if openai_wire {
        request.max_output_tokens
    } else if descriptor.is_some() || request.max_output_tokens.is_some() {
        Some(
            request
                .max_output_tokens
                .unwrap_or(crate::context::DEFAULT_RESERVED_OUTPUT_TOKENS),
        )
    } else {
        None
    };
    if let Some(limit) = limit {
        let limit = limit.min(
            descriptor
                .as_ref()
                .map_or(u64::MAX, |model| model.max_output_tokens),
        );
        if limit == 0 {
            return Err(BackendError::Configuration(
                "output token limit must be positive".into(),
            ));
        }
        if wire_api == OpenAiWireApi::GoogleGenerativeAi {
            body["generationConfig"]["maxOutputTokens"] = json!(limit);
        } else {
            body[if wire_api == OpenAiWireApi::Responses {
                "max_output_tokens"
            } else if wire_api == OpenAiWireApi::AnthropicMessages {
                "max_tokens"
            } else {
                "max_completion_tokens"
            }] = json!(limit);
        }
    }
    if let Some(format) = request.response_format {
        if wire_api == OpenAiWireApi::Responses {
            let format = if format["type"] == "json_schema" {
                let mut schema = format["json_schema"].clone();
                schema["type"] = json!("json_schema");
                schema
            } else {
                format.clone()
            };
            if body.get("text").is_none() {
                body["text"] = json!({});
            }
            body["text"]["format"] = format;
        } else {
            body["response_format"] = format.clone();
        }
    }
    if let Some(route) = route {
        if wire_api != OpenAiWireApi::GoogleGenerativeAi {
            body["stream"] = json!(route.streaming());
            if !route.streaming() {
                body.as_object_mut()
                    .expect("request object")
                    .remove("stream_options");
            }
        }
        match route.dialect() {
            Dialect::Codex => {
                body["stream"] = json!(true);
                body["store"] = json!(false);
                if body.get("instructions").is_none() {
                    body["instructions"] = json!("");
                }
            }
            Dialect::DeepSeek => {
                if wire_api == OpenAiWireApi::ChatCompletions {
                    body.as_object_mut()
                        .expect("request object")
                        .remove("metadata");
                    if let Some(limit) = body
                        .as_object_mut()
                        .expect("request object")
                        .remove("max_completion_tokens")
                    {
                        body["max_tokens"] = limit;
                    }
                    if let Some(effort) = reasoning_effort {
                        let effort = deepseek_effort(effort)?;
                        body["thinking"] =
                            json!({"type":if effort == "none" { "disabled" } else { "enabled" }});
                        body["reasoning_effort"] = json!(effort);
                    }
                } else if wire_api == OpenAiWireApi::Responses {
                    if let Some(effort) = reasoning_effort {
                        body["reasoning"]["effort"] = json!(deepseek_effort(effort)?);
                    }
                    let object = body.as_object_mut().expect("request object");
                    object.remove("store");
                    object.remove("metadata");
                }
            }
            Dialect::Default => (),
        }
    }
    Ok(body)
}

pub(crate) fn read_response_for_route(
    body: &mut ureq::Body,
    sse: bool,
    options: &RequestOptions<'_>,
    route: &ValidatedRoute,
    cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<Completion, BackendError> {
    validate_route_options(options, route)?;
    if route.streaming() != sse {
        return Err(BackendError::InvalidResponse(
            "response streaming mode differs from validated route".into(),
        ));
    }
    if options.wire_api == OpenAiWireApi::Responses {
        return responses::read_response_for_route(body, sse, cancelled, sink);
    }
    if options.wire_api == OpenAiWireApi::AnthropicMessages && route.dialect() == Dialect::DeepSeek
    {
        return anthropic::read_response_deepseek(
            body,
            sse,
            route.provider(),
            options.model,
            options.capabilities,
            cancelled,
            sink,
        );
    }
    let deepseek_chat =
        route.dialect() == Dialect::DeepSeek && route.protocol() == Protocol::ChatCompletions;
    let mut terminal = None;
    let completion = read_response(body, sse, options, cancelled, &mut |event| {
        if deepseek_chat && matches!(event, ProviderEvent::Completed { .. }) {
            terminal = Some(event);
            Ok(())
        } else {
            sink(event)
        }
    })?;
    if deepseek_chat && unsupported_deepseek_chat_details(&completion.annotations) {
        return Err(BackendError::InvalidResponse(
            "DeepSeek Chat returned unsupported opaque reasoning details".into(),
        ));
    }
    if let Some(event) = terminal {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        sink(event)?;
    }
    Ok(completion)
}

pub(crate) fn read_response(
    body: &mut ureq::Body,
    sse: bool,
    options: &RequestOptions<'_>,
    cancelled: &dyn Fn() -> bool,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<Completion, BackendError> {
    let provider = options.provider();
    match options.wire_api {
        OpenAiWireApi::GoogleGenerativeAi => google::read_response(
            body,
            sse,
            provider,
            options.model,
            options.capabilities,
            cancelled,
            sink,
        ),
        OpenAiWireApi::AnthropicMessages => anthropic::read_response(
            body,
            sse,
            provider,
            options.model,
            options.capabilities,
            cancelled,
            sink,
        ),
        OpenAiWireApi::ChatCompletions => {
            chat::read_response(body, sse, provider, options.model, cancelled, sink)
        }
        OpenAiWireApi::Responses => responses::read_response(
            body,
            options.capabilities.streaming,
            options.provider.is_some(),
            cancelled,
            sink,
        ),
    }
}

fn extract_annotations(payload: &Value) -> Vec<Value> {
    let mut annotations = Vec::new();
    if let Some(values) = payload.get("annotations").and_then(Value::as_array) {
        annotations.extend(values.iter().cloned());
    }
    if let Some(output) = payload.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    if let Some(values) = part.get("annotations").and_then(Value::as_array) {
                        annotations.extend(values.iter().cloned());
                    }
                }
            }
        }
    }
    annotations
}

fn read_bounded_json_body(
    body: &mut ureq::Body,
    cancelled: &dyn Fn() -> bool,
) -> Result<Value, BackendError> {
    use std::io::Read;
    let mut reader = body.with_config().limit(MAX_RESPONSE_BYTES as u64).reader();
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        if cancelled() {
            return Err(BackendError::Cancelled);
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if is_recv_body_poll_timeout(&error) => continue,
            Err(error) => return Err(BackendError::Transport(error.to_string())),
        }
    }
    if cancelled() {
        return Err(BackendError::Cancelled);
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| BackendError::InvalidResponse(format!("invalid JSON response: {error}")))
}

fn is_recv_body_poll_timeout(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<ureq::Error>())
        .is_some_and(|error| matches!(error, ureq::Error::Timeout(ureq::Timeout::RecvBody)))
}

fn emit_completion_events(
    completion: &Completion,
    sink: &mut dyn FnMut(ProviderEvent) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if let Some(response_id) = completion.response_id.clone() {
        sink(ProviderEvent::ResponseCreated {
            response_id: Some(response_id),
            model: completion.model.clone(),
        })?;
    }
    if !completion.content.is_empty() {
        sink(ProviderEvent::TextDelta {
            delta: completion.content.clone(),
        })?;
        sink(ProviderEvent::TextDone {
            text: completion.content.clone(),
        })?;
    }
    if let Some(refusal) = completion.refusal.clone() {
        sink(ProviderEvent::Refusal { text: refusal })?;
    }
    for call in &completion.tool_calls {
        sink(ProviderEvent::ToolCallDone { call: call.clone() })?;
    }
    if let Some(usage) = completion.usage {
        sink(ProviderEvent::Usage { usage })?;
    }
    sink(ProviderEvent::Completed {
        response_id: completion.response_id.clone(),
        model: completion.model.clone(),
    })
}

fn parse_tool_call(id: &str, name: &str, arguments: &str) -> Result<ToolCall, BackendError> {
    let arguments = serde_json::from_str(arguments).map_err(|error| {
        BackendError::InvalidResponse(format!("invalid tool call arguments: {error}"))
    })?;
    Ok(ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments,
    })
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let input = value
        .get("prompt_tokens")
        .or_else(|| value.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = value
        .get("completion_tokens")
        .or_else(|| value.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input.saturating_add(output));
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
    })
}

fn validate_response_format(format: &Value) -> Result<(), BackendError> {
    if serde_json::to_vec(format).map_or(true, |bytes| bytes.len() > 64 * 1024)
        || !format.is_object()
        || !matches!(format["type"].as_str(), Some("json_object" | "json_schema"))
        || (format["type"] == "json_schema"
            && (!format["json_schema"].is_object()
                || !format["json_schema"]["schema"].is_object()
                || !format["json_schema"]["name"]
                    .as_str()
                    .is_some_and(|name| !name.is_empty() && name.len() <= 64)))
    {
        return Err(BackendError::Configuration(
            "structured output format is invalid or too large".into(),
        ));
    }
    Ok(())
}
