import { createServer } from "node:http";

const PORT = Number(process.env.MOCK_PORT ?? 18765);
const WORD_DELAY_MS = 25;
const SMALL_MODEL_BODY_LIMIT = 30000;

const SAMPLE_ANSWER = `Компоновщик не находит **MSVC**, поэтому сборка падает.

1. Установи Build Tools с компонентом C++.
2. Открой новый терминал.

\`\`\`rust
fn main() {
    println!("Сборка работает");
}
\`\`\`

| Шаг | Статус |
| --- | ------ |
| Установка | готово |
| Проверка | готово |

Подробности есть в [документации](https://www.rust-lang.org/).`;

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function tokensOf(text) {
  return text.match(/\S+\s*|\n+/g) ?? [];
}

function readBody(request) {
  return new Promise((resolve) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => resolve(body));
  });
}

function lastUserText(messages) {
  const last = messages[messages.length - 1];
  if (typeof last?.content === "string") {
    return last.content;
  }
  return last?.content?.find((part) => part.type === "text")?.text ?? "";
}

async function streamOpenAi(response, parsed) {
  response.writeHead(200, { "content-type": "text/event-stream" });
  const prompt = lastUserText(parsed.messages);
  for (const word of tokensOf(answerFor(prompt, parsed.model))) {
    if (response.destroyed) {
      return;
    }
    response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: word }, finish_reason: null }] })}\n\n`);
    await sleep(WORD_DELAY_MS);
  }
  response.write(`data: ${JSON.stringify({ choices: [{ delta: {}, finish_reason: "stop" }] })}\n\n`);
  response.write(`data: ${JSON.stringify({ choices: [], usage: { prompt_tokens: 120, completion_tokens: 64 } })}\n\n`);
  response.write("data: [DONE]\n\n");
  response.end();
}

async function streamAnthropic(response, parsed) {
  response.writeHead(200, { "content-type": "text/event-stream" });
  const send = (type, payload) => response.write(`event: ${type}\ndata: ${JSON.stringify({ type, ...payload })}\n\n`);
  send("message_start", { message: { usage: { input_tokens: 90, cache_read_input_tokens: 30 } } });
  send("content_block_start", { index: 0, content_block: { type: "thinking", thinking: "" } });
  send("content_block_delta", { index: 0, delta: { type: "thinking_delta", thinking: "Проверяю, что важно для ответа." } });
  send("content_block_stop", { index: 0 });
  send("content_block_start", { index: 1, content_block: { type: "text", text: "" } });
  for (const word of tokensOf(answerFor(lastUserText(parsed.messages), parsed.model))) {
    if (response.destroyed) {
      return;
    }
    send("content_block_delta", { index: 1, delta: { type: "text_delta", text: word } });
    await sleep(WORD_DELAY_MS);
  }
  send("content_block_stop", { index: 1 });
  send("message_delta", { delta: { stop_reason: "end_turn" }, usage: { output_tokens: 64 } });
  send("message_stop", {});
  response.end();
}

const WORKSPACE_REPLY_MARK = "Готово: инструменты отработали";

function toolPlanFor(prompt) {
  const folderMatch = prompt.match(/инструменты: (\S+)/);
  if (!folderMatch) {
    return null;
  }
  return folderMatch[1].split(",").map((entry) => {
    const [name, argument, firstText, secondText] = entry.split("=");
    if (name === "read_file") {
      return { name, input: { path: argument } };
    }
    if (name === "write_file") {
      return { name, input: { path: argument, content: firstText ?? "записано моделью" } };
    }
    if (name === "edit_file") {
      return { name, input: { path: argument, old_string: firstText, new_string: secondText } };
    }
    if (name === "run_command") {
      return { name, input: { command: decodeURIComponent(argument) } };
    }
    if (name === "read_skill") {
      return { name, input: { name: argument } };
    }
    return { name, input: { text: argument ?? "" } };
  });
}

function pendingToolPlan(parsed) {
  const originalPrompt = parsed.messages.findLast((message) => message.role === "user" && typeof message.content === "string")?.content ?? "";
  const plan = toolPlanFor(originalPrompt);
  if (!plan) {
    return null;
  }
  const finishedCalls = parsed.messages.filter((message) => message.role === "tool").length;
  return finishedCalls < plan.length ? { call: plan[finishedCalls], index: finishedCalls } : { call: null, index: finishedCalls };
}

function toolResultsOf(parsed) {
  return parsed.messages.filter((message) => message.role === "tool").map((message) => message.content);
}

async function streamToolCall(response, pending) {
  response.writeHead(200, { "content-type": "text/event-stream" });
  const write = (delta, finish = null) => response.write(`data: ${JSON.stringify({ choices: [{ delta, finish_reason: finish }] })}\n\n`);
  const id = `call-${pending.index}`;
  write({ content: `Шаг ${pending.index + 1}.` });
  write({ tool_calls: [{ index: 0, id, function: { name: pending.call.name, arguments: "" } }] });
  const encoded = JSON.stringify(pending.call.input);
  const middle = Math.floor(encoded.length / 2);
  write({ tool_calls: [{ index: 0, function: { arguments: encoded.slice(0, middle) } }] });
  write({ tool_calls: [{ index: 0, function: { arguments: encoded.slice(middle) } }] });
  write({}, "tool_calls");
  response.write(`data: ${JSON.stringify({ choices: [], usage: { prompt_tokens: 50, completion_tokens: 10 } })}\n\n`);
  response.write("data: [DONE]\n\n");
  response.end();
}

async function streamToolSummary(response, parsed) {
  response.writeHead(200, { "content-type": "text/event-stream" });
  const summary = `${WORKSPACE_REPLY_MARK}\n\n${toolResultsOf(parsed).map((result) => `- ${result}`).join("\n")}`;
  response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: summary }, finish_reason: null }] })}\n\n`);
  response.write(`data: ${JSON.stringify({ choices: [{ delta: {}, finish_reason: "stop" }] })}\n\n`);
  response.write(`data: ${JSON.stringify({ choices: [], usage: { prompt_tokens: 60, completion_tokens: 12 } })}\n\n`);
  response.write("data: [DONE]\n\n");
  response.end();
}

function answerFor(prompt, model) {
  if (prompt.includes("длинный")) {
    return Array.from({ length: 10 },(_, index) => `Абзац ${index + 1}. ${SAMPLE_ANSWER}`).join("\n\n");
  }
  return `Отвечает **${model}**.\n\n${SAMPLE_ANSWER}`;
}

let lastRequestSummary = {};

function summarizeRequest(parsed) {
  const contentParts = parsed.messages.flatMap((message) => (Array.isArray(message.content) ? message.content : []));
  return {
    imageCount: contentParts.filter((part) => part.type === "image_url").length,
    emptyTextParts: contentParts.filter((part) => part.type === "text" && part.text === "").length,
    effort: parsed.output_config?.effort ?? parsed.reasoning_effort ?? parsed.reasoning?.effort ?? null,
    toolNames: (parsed.tools ?? []).map((tool) => tool.function?.name ?? tool.name),
    system: parsed.messages.find((message) => message.role === "system")?.content ?? parsed.system ?? null,
    toolMessages: parsed.messages.filter((message) => message.role === "tool").length,
  };
}

function summarizeAnthropicRequest(parsed) {
  return { effort: parsed.output_config?.effort ?? null, toolNames: (parsed.tools ?? []).map((tool) => tool.name), system: parsed.system ?? null };
}

const MODELS = {
  data: [
    { id: "mock-fast", display_name: "Mock Fast", max_tokens: 8000 },
    { id: "mock-smart", display_name: "Mock Smart", max_tokens: 8000 },
    { id: "mock-local", display_name: "Mock Local", max_tokens: 8000 },
    { id: "mock-small", display_name: "Mock Small", max_tokens: 8000 },
    {
      id: "mock-effort",
      display_name: "Mock Effort",
      max_tokens: 8000,
      capabilities: { effort: { supported: true, low: { supported: true }, high: { supported: true }, max: { supported: true } } },
    },
    { id: "mock-no-tools", display_name: "Mock No Tools", max_tokens: 8000 },
    { id: "text-embedding-mock" },
  ],
};

process.on("uncaughtException", (error) => console.error("uncaught", error));
process.on("unhandledRejection", (error) => console.error("unhandled", error));

const server = createServer(async (request, response) => {
  console.log(new Date().toISOString(), request.method, request.url);
  const url = request.url ?? "";
  const authorized = request.headers.authorization === "Bearer good-key" || request.headers["x-api-key"] === "good-key";
  const needsKey = url.startsWith("/secure");
  if (needsKey && !authorized) {
    response.writeHead(401, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: { message: "invalid key" } }));
    return;
  }
  const path = url.replace(/^\/secure/, "").split("?")[0];
  if (request.method === "GET" && (path === "/v1/models")) {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify(MODELS));
    return;
  }
  if (request.method === "GET" && path === "/v1/last-request") {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify(lastRequestSummary));
    return;
  }
  if (request.method === "POST" && path === "/v1/chat/completions") {
    const body = await readBody(request);
    const parsed = JSON.parse(body);
    lastRequestSummary = summarizeRequest(parsed);
    if (parsed.model === "mock-small" && body.length > SMALL_MODEL_BODY_LIMIT) {
      response.writeHead(400, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: { message: "This model's maximum context length is exceeded" } }));
      return;
    }
    if (lastUserText(parsed.messages).includes("ошибка")) {
      response.writeHead(500, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: { message: "upstream exploded" } }));
      return;
    }
    if (parsed.model === "mock-no-tools" && parsed.tools?.length) {
      response.writeHead(400, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: { message: "This model does not support tools" } }));
      return;
    }
    const pending = pendingToolPlan(parsed);
    if (pending?.call && parsed.tools?.length) {
      await streamToolCall(response, pending);
      return;
    }
    if (pending && !pending.call) {
      await streamToolSummary(response, parsed);
      return;
    }
    await streamOpenAi(response, parsed);
    return;
  }
  if (request.method === "POST" && path === "/v1/messages") {
    const parsed = JSON.parse(await readBody(request));
    lastRequestSummary = summarizeAnthropicRequest(parsed);
    await streamAnthropic(response, parsed);
    return;
  }
  response.writeHead(404);
  response.end();
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`mock provider on http://127.0.0.1:${PORT}/v1`);
});
