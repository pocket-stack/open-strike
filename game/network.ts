// One bounded exchange in flight. Socket/file work belongs to the native
// PocketJS worker and Companion; the guest moves serialized input/snapshots.
import { offload } from "@pocketjs/framework/offload";
import { strike } from "./sdk.ts";
let ticket = 0, started = 0, serial = 0, previousTime = 0;
strike.onTick((state) => {
  if (!state.network) {
    if (ticket) offload().cancel(ticket);
    ticket = 0; serial++; previousTime = state.time;
    return;
  }
  const client = offload();
  if (state.time < previousTime) { if (ticket) client.cancel(ticket); ticket = 0; serial++; }
  previousTime = state.time;
  if (ticket && state.time - started > 1.5) {
    client.cancel(ticket); ticket = 0; serial++; strike.networkReply("");
  }
  if (ticket || !state.networkRequest || !client.connected()) return;
  const generation = ++serial;
  started = state.time;
  ticket = client.request("strike.exchange", state.networkRequest, (result) => {
    if (generation !== serial) return;
    ticket = 0;
    strike.networkReply(result.ok ? result.value : "!" + result.error);
  });
});
