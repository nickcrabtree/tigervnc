# PersistentCache round-trip test notes

This file exists to document the decoder-level PC MISS→INIT→HIT test located under `rfb-encodings/tests/`.
It validates client MISS reporting, INIT store, subsequent HIT, and ARC eviction surfacing without requiring a live server.
