"""Numbers for one Claude Code run: summary.py <stream.jsonl> <calls.jsonl>.

The stream is `claude -p --output-format stream-json --verbose`; the calls log is the
server's own line per tool call (tool, ok, microseconds).
"""
import collections
import json
import sys

stream, calls = sys.argv[1], sys.argv[2]
init, result, uses = {}, {}, []
for line in open(stream):
    e = json.loads(line)
    if e.get("type") == "system" and e.get("subtype") == "init":
        init = e
    elif e.get("type") == "assistant":
        for c in e["message"].get("content", []):
            if c.get("type") == "tool_use":
                uses.append(c["name"])
    elif e.get("type") == "result":
        result = e
log = [json.loads(l) for l in open(calls)] if calls and __import__("os").path.exists(calls) else []
by = collections.Counter(c["tool"] for c in log)
errors = [c for c in log if not c["ok"]]
u = result.get("usage", {})
print(f"model: {init.get('model')}; mcp: {[ (s['name'], s['status']) for s in init.get('mcp_servers', []) ]}; tools offered: {len(init.get('tools', []))}")
print(f"result: {result.get('subtype')}, is_error {result.get('is_error')}, turns {result.get('num_turns')}, "
      f"wall {result.get('duration_ms', 0) / 1000:.1f} s (api {result.get('duration_api_ms', 0) / 1000:.1f} s)")
print(f"tokens: input {u.get('input_tokens')}, cache read {u.get('cache_read_input_tokens')}, "
      f"cache write {u.get('cache_creation_input_tokens')}, output {u.get('output_tokens')}; "
      f"cost as reported {result.get('total_cost_usd')}")
print(f"tool calls: {len(uses)} requested, {len(log)} reached the server, {len(errors)} returned an error; "
      f"server time {sum(c['us'] for c in log) / 1000:.1f} ms in all")
print("by tool: " + ", ".join(f"{k} {v}" for k, v in by.most_common()))
for c in errors:
    print(f"  error from {c['tool']}: {c['result'][:160]}")
print(f"done called: {'done' in by}; saved: {by.get('save', 0)} times")
print("final text: " + (result.get("result") or "").strip().replace("\n", " ")[:300])
