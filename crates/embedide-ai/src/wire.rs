//! HTTP adapters for the Anthropic Messages API and OpenAI-style Chat
//! Completions (OpenAI, Moonshot, xAI). No automatic retries: a retried
//! request can be billed twice, so rate limits are reported to the caller.
use crate::{
    providers::Protocol, Backend, Cancel, Error, ModelInfo, ProviderId, Request, Response, Result,
    Role, Secret, Usage,
};
use serde_json::{json, Value};
use std::{io::BufRead, time::Duration};

pub(crate) struct HttpBackend {
    provider: ProviderId,
    base: String,
    key: Secret,
    agent: ureq::Agent,
}

impl HttpBackend {
    pub(crate) fn new(provider: ProviderId, base: String, key: Secret) -> Result<Self> {
        let base = base.trim_end_matches('/').to_string();
        if !(base.starts_with("https://")
            || base.starts_with("http://127.0.0.1")
            || base.starts_with("http://localhost"))
        {
            return Err(Error::Config(format!(
                "Refusing non-HTTPS AI endpoint {base} (plain HTTP is allowed only for localhost)"
            )));
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(120))
            .redirects(0)
            .build();
        Ok(Self {
            provider,
            base,
            key,
            agent,
        })
    }

    fn protocol(&self) -> Protocol {
        self.provider.info().protocol
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let url = format!("{}{}", self.base, path);
        let r = self
            .agent
            .request(method, &url)
            .set("accept", "application/json, text/event-stream");
        match self.protocol() {
            Protocol::AnthropicMessages => r
                .set("x-api-key", self.key.expose())
                .set("anthropic-version", "2023-06-01"),
            Protocol::ChatCompletions => {
                r.set("authorization", &format!("Bearer {}", self.key.expose()))
            }
        }
    }

    fn map_err(&self, e: ureq::Error) -> Error {
        match e {
            ureq::Error::Status(code, resp) => {
                let retry = resp
                    .header("retry-after")
                    .and_then(|v| v.trim().parse::<u64>().ok());
                let body = resp.into_string().unwrap_or_default();
                let msg = serde_json::from_str::<Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(|m| m.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| body.chars().take(300).collect());
                let msg = self.key.redact(&msg);
                match code {
                    401 | 403 => Error::Auth(msg),
                    429 => Error::RateLimited(retry, msg),
                    _ => Error::Provider(code, msg),
                }
            }
            ureq::Error::Transport(t) => Error::Network(self.key.redact(&t.to_string())),
        }
    }

    fn body(&self, req: &Request) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| json!({"role": match m.role { Role::User => "user", Role::Assistant => "assistant" }, "content": m.content}))
            .collect();
        match self.protocol() {
            Protocol::AnthropicMessages => {
                let mut b = json!({"model": req.model, "max_tokens": req.max_tokens, "messages": messages, "stream": req.stream});
                if let Some(s) = &req.system {
                    b["system"] = json!(s);
                }
                if let Some(t) = req.temperature {
                    b["temperature"] = json!(t);
                }
                b
            }
            Protocol::ChatCompletions => {
                let mut all = Vec::new();
                if let Some(s) = &req.system {
                    all.push(json!({"role": "system", "content": s}));
                }
                all.extend(messages);
                let mut b = json!({"model": req.model, "messages": all, "stream": req.stream});
                // OpenAI's current models take max_completion_tokens; Moonshot/xAI document max_tokens
                let cap = if self.provider == ProviderId::Openai {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                };
                b[cap] = json!(req.max_tokens);
                if req.stream {
                    b["stream_options"] = json!({"include_usage": true});
                }
                if let Some(t) = req.temperature {
                    b["temperature"] = json!(t);
                }
                b
            }
        }
    }
}

fn parse_full(protocol: Protocol, v: &Value) -> Response {
    match protocol {
        Protocol::AnthropicMessages => Response {
            text: v["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<String>()
                })
                .unwrap_or_default(),
            usage: Usage {
                input_tokens: v["usage"]["input_tokens"].as_u64(),
                output_tokens: v["usage"]["output_tokens"].as_u64(),
            },
            stop_reason: v["stop_reason"].as_str().map(str::to_owned),
            model: v["model"].as_str().map(str::to_owned),
        },
        Protocol::ChatCompletions => Response {
            text: v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            usage: Usage {
                input_tokens: v["usage"]["prompt_tokens"].as_u64(),
                output_tokens: v["usage"]["completion_tokens"].as_u64(),
            },
            stop_reason: v["choices"][0]["finish_reason"].as_str().map(str::to_owned),
            model: v["model"].as_str().map(str::to_owned),
        },
    }
}

impl Backend for HttpBackend {
    fn label(&self) -> String {
        format!("{} (API key)", self.provider.info().name)
    }

    fn send(
        &self,
        req: &Request,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Response> {
        if crate::offline() {
            return Err(Error::Offline);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let path = match self.protocol() {
            Protocol::AnthropicMessages => "/v1/messages",
            Protocol::ChatCompletions => "/chat/completions",
        };
        let resp = self
            .request("POST", path)
            .set("content-type", "application/json")
            .send_string(&self.body(req).to_string())
            .map_err(|e| self.map_err(e))?;
        if !req.stream {
            let v: Value = serde_json::from_reader(resp.into_reader())
                .map_err(|e| Error::Network(format!("Invalid JSON from provider: {e}")))?;
            return Ok(parse_full(self.protocol(), &v));
        }
        // Server-sent events
        let mut out = Response {
            text: String::new(),
            usage: Usage::default(),
            stop_reason: None,
            model: None,
        };
        let reader = std::io::BufReader::new(resp.into_reader());
        for line in reader.lines() {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let line = line.map_err(|e| Error::Network(e.to_string()))?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            match self.protocol() {
                Protocol::AnthropicMessages => match v["type"].as_str() {
                    Some("message_start") => {
                        out.model = v["message"]["model"].as_str().map(str::to_owned);
                        out.usage.input_tokens = v["message"]["usage"]["input_tokens"].as_u64();
                    }
                    Some("content_block_delta") => {
                        if let Some(t) = v["delta"]["text"].as_str() {
                            out.text.push_str(t);
                            on_delta(t);
                        }
                    }
                    Some("message_delta") => {
                        out.stop_reason = v["delta"]["stop_reason"]
                            .as_str()
                            .map(str::to_owned)
                            .or(out.stop_reason.take());
                        if let Some(o) = v["usage"]["output_tokens"].as_u64() {
                            out.usage.output_tokens = Some(o);
                        }
                    }
                    Some("error") => {
                        return Err(Error::Provider(
                            0,
                            self.key
                                .redact(v["error"]["message"].as_str().unwrap_or("stream error")),
                        ));
                    }
                    _ => {}
                },
                Protocol::ChatCompletions => {
                    if out.model.is_none() {
                        out.model = v["model"].as_str().map(str::to_owned);
                    }
                    if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                        out.text.push_str(t);
                        on_delta(t);
                    }
                    if let Some(r) = v["choices"][0]["finish_reason"].as_str() {
                        out.stop_reason = Some(r.to_string());
                    }
                    if v["usage"].is_object() {
                        out.usage = Usage {
                            input_tokens: v["usage"]["prompt_tokens"].as_u64(),
                            output_tokens: v["usage"]["completion_tokens"].as_u64(),
                        };
                    }
                }
            }
        }
        Ok(out)
    }

    fn models(&self) -> Result<Vec<ModelInfo>> {
        if crate::offline() {
            return Err(Error::Offline);
        }
        let path = match self.protocol() {
            Protocol::AnthropicMessages => "/v1/models",
            Protocol::ChatCompletions => "/models",
        };
        let resp = self
            .request("GET", path)
            .call()
            .map_err(|e| self.map_err(e))?;
        let v: Value = serde_json::from_reader(resp.into_reader())
            .map_err(|e| Error::Network(format!("Invalid JSON from provider: {e}")))?;
        let mut list: Vec<ModelInfo> = v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                m["id"].as_str().map(|id| ModelInfo {
                    id: id.to_string(),
                    name: m["display_name"].as_str().map(str::to_owned),
                })
            })
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(list)
    }
}
