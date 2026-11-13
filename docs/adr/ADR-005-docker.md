# ADR-005: Read-only Docker integration

- **Status:** Accepted
- **Context:** Docker metadata can help developers understand local resource use, but mutating containers risks data loss.
- **Decision:** MVP integration is strictly read-only inventory and recommendations.
- **Consequences:** No prune, stop, removal, volume deletion or other mutation command. External responses remain untrusted. Implemented so far: the Developer page's disk-use reading, one read-only `GET /system/df` ([ADR-011](ADR-011-docker-disk-use.md)).
