// Mirrors @forge/tunnel's out/sandbox/sandbox-runner.js (13 lines): require the loader, call
// `main(lambdaEvent, lambdaContext)`. __forge_tunnel__ makes the real wrapper write console logs
// to stdout as JSON lines instead of shipping them to the proxy's /logs.
global.__forge_tunnel__ = true;
const handler = require(process.argv[2]).main;
process.on('message', async ({ lambdaEvent, deadline }) => {
  const result = await handler(lambdaEvent, { awsRequestId: 'emulator', getRemainingTimeInMillis: () => deadline - Date.now() });
  process.send({ result });
});
