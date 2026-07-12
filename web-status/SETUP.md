# Symphony Status — finish setup

App is deployed at:

**https://symphony-status.vercel.app**

## 1. Add env vars in Vercel (required)

Open:
https://vercel.com/oversparklings-projects/symphony-status/settings/environment-variables

Add for Production (and Preview):

| Name | Value |
|---|---|
| `STATUS_TOKEN` | a long random secret (keep private) |

Generate one locally with `openssl rand -hex 32`.

## 2. Create a Blob store (required)

Open Storage on the project and create a **Blob** store linked to `symphony-status`.
That injects `BLOB_READ_WRITE_TOKEN` automatically.

## 3. Redeploy

After env + Blob are set, redeploy from the Vercel Deployments tab.

## 4. Point Symphony desktop at it

In Symphony → Settings → Worker:

- **Web status URL:** `https://symphony-status.vercel.app`
- **Web status token:** same `STATUS_TOKEN` as above

Save, rebuild/restart Symphony desktop so the sync loop is active, and keep the worker running. Unlock the phone dashboard with that token.
