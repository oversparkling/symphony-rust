import { requireAuth } from "@/lib/auth";
import { getStatus, putStatus } from "@/lib/store";
import type { StatusSnapshot } from "@/lib/types";

export const runtime = "nodejs";

export async function GET(req: Request) {
  const unauthorized = requireAuth(req);
  if (unauthorized) return unauthorized;
  try {
    const status = await getStatus();
    if (!status) {
      return Response.json({ empty: true }, { status: 200 });
    }
    return Response.json(status);
  } catch (err) {
    return Response.json(
      { error: err instanceof Error ? err.message : "status read failed" },
      { status: 500 },
    );
  }
}

export async function POST(req: Request) {
  const unauthorized = requireAuth(req);
  if (unauthorized) return unauthorized;
  let body: StatusSnapshot;
  try {
    body = (await req.json()) as StatusSnapshot;
  } catch {
    return Response.json({ error: "invalid json" }, { status: 400 });
  }
  if (!body || typeof body !== "object" || !body.updatedAt || !body.worker) {
    return Response.json({ error: "invalid snapshot" }, { status: 400 });
  }
  try {
    await putStatus({
      ...body,
      updatedAt: new Date().toISOString(),
    });
  } catch (err) {
    return Response.json(
      { error: err instanceof Error ? err.message : "status write failed" },
      { status: 500 },
    );
  }
  return Response.json({ ok: true });
}
