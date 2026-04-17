---
name: Game Informer (headlines)
description: Game Informer / GI news — fetch HTML, reply in plain human language (no raw HTML/XML dumps).
tags: [gameinformer, game-informer]
---

## When to use

The user asks about **Game Informer**, **GI**, **gameinformer.com**, or similar site news.

## Rules (fast, clean)

1. **Never** `fetch` `*.xml`, `*/rss*`, or `news.xml` / feed URLs — you get raw RSS/XML and a huge, ugly reply. Use **HTML** only.
2. **`fetch` once**: `https://www.gameinformer.com/` **or** `https://www.gameinformer.com/news` (pick one; do not chain both unless the user asked for detail).
3. In `<pengine_reply>`: write for a **human reader** only — short sentences, headlines you’d say out loud. **Do not** paste raw HTML, XML, tags, attributes, or big chunks from the `fetch` tool output. Use the fetch result **only to read**; then paraphrase.
4. **Up to 8** stories as markdown links: `- [Title](url)` (title in your own words if needed). No code blocks of page source. One brief intro line is enough. End with **Quellen:** and the exact page URL you fetched.

Do **not** use `brave_web_search` for this unless the user explicitly asked to search the web.
