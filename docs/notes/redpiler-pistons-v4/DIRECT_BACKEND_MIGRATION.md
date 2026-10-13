# Superseded migration plan

The generalized-Direct proposal was replaced on 2026-10-13 by [one compiler graph, an extensible pass pipeline, and one Graph runtime](GRAPH_PIPELINE_MIGRATION.md).

Use that plan and its G1–G7 step documents. GraphBackend remains the executor; Direct and Native are retained during migration and retired after the coverage/admission gate. Do not follow the former instructions to move Graph execution into Direct or remove `Runtime::Graph`.
