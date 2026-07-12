import { list, put } from "@vercel/blob";
import type { CommandEnvelope, RemoteCommand, StatusSnapshot } from "./types";

const STATUS_KEY = "symphony/status.json";
const COMMANDS_KEY = "symphony/commands.json";

function requireBlobToken() {
  if (!process.env.BLOB_READ_WRITE_TOKEN) {
    throw new Error(
      "BLOB_READ_WRITE_TOKEN is not configured. Create a Blob store on the Vercel project.",
    );
  }
}

async function readJsonBlob<T>(pathname: string, fallback: T): Promise<T> {
  requireBlobToken();
  const { blobs } = await list({ prefix: pathname, limit: 1 });
  const blob = blobs.find((item) => item.pathname === pathname);
  if (!blob) return fallback;
  const res = await fetch(blob.url, { cache: "no-store" });
  if (!res.ok) return fallback;
  return (await res.json()) as T;
}

async function writeJsonBlob(pathname: string, value: unknown) {
  requireBlobToken();
  await put(pathname, JSON.stringify(value), {
    access: "public",
    addRandomSuffix: false,
    allowOverwrite: true,
    contentType: "application/json",
  });
}

export async function getStatus(): Promise<StatusSnapshot | null> {
  return readJsonBlob<StatusSnapshot | null>(STATUS_KEY, null);
}

export async function putStatus(snapshot: StatusSnapshot): Promise<void> {
  await writeJsonBlob(STATUS_KEY, snapshot);
}

export async function getCommands(): Promise<RemoteCommand[]> {
  const envelope = await readJsonBlob<CommandEnvelope>(COMMANDS_KEY, {
    commands: [],
  });
  return envelope.commands ?? [];
}

export async function putCommands(commands: RemoteCommand[]): Promise<void> {
  const trimmed = commands.slice(-100);
  await writeJsonBlob(COMMANDS_KEY, {
    commands: trimmed,
  } satisfies CommandEnvelope);
}

export async function enqueueCommand(
  command: Omit<RemoteCommand, "status" | "createdAt"> & {
    createdAt?: string;
  },
): Promise<RemoteCommand> {
  const commands = await getCommands();
  const next: RemoteCommand = {
    ...command,
    createdAt: command.createdAt ?? new Date().toISOString(),
    status: "pending",
  };
  commands.push(next);
  await putCommands(commands);
  return next;
}

export async function ackCommands(
  acks: Array<{ id: string; ok: boolean; error?: string }>,
): Promise<RemoteCommand[]> {
  const commands = await getCommands();
  const now = new Date().toISOString();
  const byId = new Map(acks.map((ack) => [ack.id, ack]));
  for (const command of commands) {
    const ack = byId.get(command.id);
    if (!ack) continue;
    command.status = ack.ok ? "acked" : "failed";
    command.ackedAt = now;
    command.error = ack.error;
  }
  await putCommands(commands);
  return commands;
}

export function pendingCommands(commands: RemoteCommand[]): RemoteCommand[] {
  return commands.filter((command) => command.status === "pending");
}
