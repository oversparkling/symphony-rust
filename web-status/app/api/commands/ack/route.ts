import { requireAuth } from "@/lib/auth";
import { ackCommands } from "@/lib/store";

export const runtime = "nodejs";

export async function POST(req: Request) {
  const unauthorized = requireAuth(req);
  if (unauthorized) return unauthorized;
  let body: { acks?: Array<{ id: string; ok: boolean; error?: string }> };
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: "invalid json" }, { status: 400 });
  }
  if (!Array.isArray(body.acks)) {
    return Response.json({ error: "acks required" }, { status: 400 });
  }
  const commands = await ackCommands(body.acks);
  return Response.json({ ok: true, commands });
}
