# Security Policy — moved

**The security policy for this repository is [`/SECURITY.md`](../SECURITY.md) at the repository
root.** Please report vulnerabilities using the channel described there.

## Why this file is a pointer

This file previously carried its own reporting address, `security@wrfcoin.example.com`.
**`example.com` is reserved by [RFC 2606](https://www.rfc-editor.org/rfc/rfc2606) and cannot receive
mail** — that address was structurally undeliverable, not merely unmonitored, so a reporter who found
this file first would have had their report silently go nowhere.

It also could never have functioned as *the* policy: **GitHub surfaces a security policy only from
the repository root or `.github/`, never from `docs/`.** Before the root file landed,
`GET /repos/wrfcoin/core4/community/profile` returned `security_policy: null` — there was no policy an
outside reporter could discover through GitHub at all.

The file is kept as a redirect rather than deleted so existing links and bookmarks resolve instead of
404ing.
