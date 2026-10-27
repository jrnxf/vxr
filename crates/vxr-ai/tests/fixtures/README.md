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
