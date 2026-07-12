import { timingSafeEqual } from "crypto";

export function requireAuth(req: Request): Response | null {
  const expected = process.env.STATUS_TOKEN;
  if (!expected) {
    return Response.json(
      { error: "STATUS_TOKEN is not configured on the server" },
      { status: 500 },
    );
  }
  const header = req.headers.get("authorization") ?? "";
  const match = /^Bearer\s+(.+)$/i.exec(header);
  const provided = match?.[1]?.trim() ?? "";
  if (!provided || !safeEqual(provided, expected)) {
    return Response.json({ error: "unauthorized" }, { status: 401 });
  }
  return null;
}

function safeEqual(a: string, b: string): boolean {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  if (left.length !== right.length) return false;
  return timingSafeEqual(left, right);
}
