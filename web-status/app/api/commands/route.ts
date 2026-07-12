import { randomUUID } from "crypto";
import { requireAuth } from "@/lib/auth";
import {
  enqueueCommand,
  getCommands,
  pendingCommands,
} from "@/lib/store";
import type { RemoteCommandType } from "@/lib/types";

export const runtime = "nodejs";

const ALLOWED = new Set<RemoteCommandType>([
  "retry_now",
  "stop_run",
  "start_worker",
  "stop_worker",
]);

export async function GET(req: Request) {
  const unauthorized = requireAuth(req);
  if (unauthorized) return unauthorized;
  const url = new URL(req.url);
  const pendingOnly = url.searchParams.get("pending") === "1";
  const commands = await getCommands();
  return Response.json({
    commands: pendingOnly ? pendingCommands(commands) : commands,
  });
}

export async function POST(req: Request) {
  const unauthorized = requireAuth(req);
  if (unauthorized) return unauthorized;
  let body: {
    type?: string;
    issueId?: string;
    runId?: string;
  };
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: "invalid json" }, { status: 400 });
  }
  const type = body.type as RemoteCommandType | undefined;
  if (!type || !ALLOWED.has(type)) {
    return Response.json({ error: "invalid command type" }, { status: 400 });
  }
  if (type === "retry_now" && !body.issueId) {
    return Response.json({ error: "issueId required" }, { status: 400 });
  }
  if (type === "stop_run" && !body.runId) {
    return Response.json({ error: "runId required" }, { status: 400 });
  }
  const command = await enqueueCommand({
    id: randomUUID(),
    type,
    issueId: body.issueId,
    runId: body.runId,
  });
  return Response.json({ ok: true, command });
}
