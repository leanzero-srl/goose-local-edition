import dgram from 'node:dgram';
import os from 'node:os';

/**
 * macOS local network privacy (Apple TN3179). The Local Network privilege belongs to the app
 * (the "responsible code"), and every child — goosed, the preflight's ping, a rank, a model copy —
 * inherits it. The system shows its alert the first time the app performs a local network
 * operation, but "macOS fails to display the local network alert when a process with a very short
 * lifespan performs a local network operation (FB16131937)" — exactly the preflight's `/sbin/ping`.
 * So MAIN, the long-lived responsible process, performs the operation TN3179 recommends at the
 * moment the feature needs the network: "connect a UDP socket to a local network address. This
 * triggers the local network alert without generating any network traffic."
 */

// TN3179's own Local Network pane is not an anchor in macOS 26.7's Privacy & Security extension
// (its anchors, read from the extension's searchTerms, stop at Privacy_*/FileVault/…): the URL
// opens Privacy & Security, where Local Network is a row.
export const LOCAL_NETWORK_SETTINGS_URL =
  'x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork';

/** RFC 1918 private ranges and IPv4 link-local: the addresses a Thunderbolt bridge or LAN uses. */
function isLocalNetworkIpv4(address: string): boolean {
  const [a, b] = address.split('.').map(Number);
  return (
    a === 10 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254)
  );
}

function toInt(address: string): number {
  return address.split('.').reduce((acc, part) => acc * 256 + Number(part), 0);
}

function toDotted(value: number): string {
  return [24, 16, 8, 0].map((shift) => Math.floor(value / 2 ** shift) % 256).join('.');
}

export interface LocalNetworkTarget {
  interfaceName: string;
  address: string;
}

/**
 * One neighbour address per local-network IPv4 subnet this Mac sits on: the subnet's first host,
 * or its second when the first is this Mac. A /31 or /32 has no neighbour to name.
 */
export function localNetworkTargets(
  interfaces: NodeJS.Dict<os.NetworkInterfaceInfo[]>
): LocalNetworkTarget[] {
  const targets: LocalNetworkTarget[] = [];
  for (const [interfaceName, infos] of Object.entries(interfaces)) {
    for (const info of infos ?? []) {
      if (info.family !== 'IPv4' || info.internal || !isLocalNetworkIpv4(info.address)) continue;
      const own = toInt(info.address);
      const mask = toInt(info.netmask);
      const network = own - (own % (2 ** 32 - mask));
      const broadcast = network + (2 ** 32 - mask) - 1;
      const first = network + 1;
      const neighbour = first === own ? first + 1 : first;
      if (neighbour >= broadcast) continue;
      targets.push({ interfaceName, address: toDotted(neighbour) });
    }
  }
  return targets;
}

export interface LocalNetworkTouch extends LocalNetworkTarget {
  /** null = the connect succeeded; otherwise the error code the OS returned. */
  error: string | null;
}

function touch(target: LocalNetworkTarget): Promise<LocalNetworkTouch> {
  return new Promise((resolve) => {
    const socket = dgram.createSocket('udp4');
    let settled = false;
    const done = (error?: NodeJS.ErrnoException | null) => {
      if (settled) return;
      settled = true;
      socket.close();
      resolve({ ...target, error: error ? (error.code ?? error.message) : null });
    };
    socket.on('error', done);
    // Port 9 is discard; a connected UDP socket sends nothing until written to. Node hands a
    // failed connect to this callback.
    socket.connect(9, target.address, done);
  });
}

/** Performs the alert-triggering connect toward every local subnet; the outcomes are data. */
export async function touchLocalNetwork(): Promise<LocalNetworkTouch[]> {
  if (process.platform !== 'darwin') return [];
  return Promise.all(localNetworkTargets(os.networkInterfaces()).map(touch));
}
