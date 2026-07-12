# Symphony Status

Personal mobile dashboard for Symphony. Deployed on Vercel.

## Setup

1. Set `STATUS_TOKEN` in the Vercel project env (Production + Preview).
2. In Symphony desktop Settings → Worker:
   - Web status URL: your deployment URL
   - Web status token: same `STATUS_TOKEN`
3. Keep Symphony running on your Mac.

## Local

```bash
cp .env.example .env.local
# edit STATUS_TOKEN
npm install
npm run dev
```
