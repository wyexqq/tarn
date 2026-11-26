import { createInterface } from "node:readline";

const TOOLS = [
  { name: "find_note", description: "Найти заметку", annotations: { readOnlyHint: true }, inputSchema: { type: "object", properties: { text: { type: "string" } } } },
  { name: "save_note", description: "Сохранить заметку", inputSchema: { type: "object", properties: { text: { type: "string" } } } },
];

function reply(id, result) {
  process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`);
}

const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  const message = JSON.parse(line);
  if (message.method === "initialize") {
    reply(message.id, { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "mock", version: "1" } });
  } else if (message.method === "tools/list") {
    reply(message.id, { tools: TOOLS });
  } else if (message.method === "tools/call") {
    reply(message.id, { content: [{ type: "text", text: `Сервер выполнил ${message.params.name}: ${message.params.arguments.text ?? ""}` }] });
  }
});
