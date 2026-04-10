# Build logs

Histórico de runs de cargo/npm/tauri/clippy/test. Cada archivo es `{tool}_{ISO timestamp}.log`,
con un alias `{tool}_latest.log` siempre apuntando al más reciente.

No se commitean al repo (ver .gitignore aquí dentro), pero sí se mantienen en disco para que
la asamblea revise patrones de regresión y rendimiento entre ciclos.
