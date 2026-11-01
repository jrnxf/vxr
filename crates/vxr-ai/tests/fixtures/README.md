# Recorded streams

Each `.sse` file is one provider's streamed answer, byte for byte, as `tests/streams.rs`
decodes it in chunks of every size.

The `*-recorded.sse` files come from the live check, which only a build with the `live`
feature of `vxr-app` has (shipped builds leave it out):

```sh
ANTHROPIC_API_KEY=... OPENAI_API_KEY=... \
  ./dev cargo run --release -p vxr-app --features live -- --live /tmp/vxr-live
```

It sends two requests on a small house through the assistant panel for each key that is set
(it skips a provider without one; it costs a few cents each), applies the change, writes
`live-<provider>.png` and each answer's raw stream as `<provider>-streams/NN.sse` into the
folder given. Copy a stream here to make it a fixture. Keys stay in memory and never reach a
stream file.

These variables change the run:

- `VXR_LIVE_KINDS=anthropic` (or `openai`, or both) runs only those providers.
- `VXR_LIVE_SCENE=/home/user/scene.vox` uses that scene instead of the house.
- `VXR_LIVE_MODEL=<model id>` asks that model of the provider instead of its default.
- `VXR_LIVE_PROMPTS="first|then"` replaces the two requests, one per `|`-separated part.
