---
"midi" = "patch"
---

Fixed midi failing to build with only its midir backend (`input_ports` came from the native backend): the transient-empty guard now reads the midir enumeration the rescan acts on (signal)
