---
"daw-standalone" = "minor"
---

Render a block's tracks across cores as soon as what feeds each is done — folders, buses, sends and their FX included — the same way live and offline, sample for sample what one thread renders; offline renders (bounces, listens, MIX_RENDER) use a pool sized to the machine (`FTS_OFFLINE_RENDER_THREADS`), CLAP instances run on any render thread (`FTS_RENDER_CLAP_SERIAL=1` keeps them on one), and every offline render reports one `render.offline` span (session)
