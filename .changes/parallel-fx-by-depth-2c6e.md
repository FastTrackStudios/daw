---
"daw-standalone" = "patch"
"builtin" = "patch"
---

FX chains run across cores by depth in the routing graph: at the start of each depth every track of it has its whole input (its children, its received sends), so the depth's chains run side by side on the lane pool, not only the tracks nothing feeds. The built-in plugins say they are safe to (unless they take a sidechain), and the render ahead's pool is half the cores (at most 15; `FTS_RENDER_AHEAD_THREADS`), not three. On Unbreakable the render ahead went from one core of FX a second to 3.6 (session)
