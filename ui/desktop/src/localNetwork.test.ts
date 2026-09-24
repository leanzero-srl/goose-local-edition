import type os from 'node:os';
import { expect, it } from 'vitest';
import { localNetworkTargets } from './localNetwork';

const v4 = (address: string, netmask: string, internal = false): os.NetworkInterfaceInfo => ({
  address,
  netmask,
  family: 'IPv4',
  mac: '00:00:00:00:00:00',
  internal,
  cidr: null,
});

it('names one neighbour on every local-network subnet this Mac sits on, never itself', () => {
  // This MacBook on 2026-09-24: the Thunderbolt /30 to the Studio, Wi-Fi, Tailscale, loopback.
  const targets = localNetworkTargets({
    lo0: [v4('127.0.0.1', '255.0.0.0', true)],
    en3: [v4('192.168.0.1', '255.255.255.252')],
    en0: [v4('192.168.8.144', '255.255.255.0')],
    utun4: [v4('100.101.12.7', '255.255.255.255')],
    bridge0: [v4('169.254.10.20', '255.255.0.0')],
  });
  expect(targets).toEqual([
    { interfaceName: 'en3', address: '192.168.0.2' },
    { interfaceName: 'en0', address: '192.168.8.1' },
    { interfaceName: 'bridge0', address: '169.254.0.1' },
  ]);
});

it('skips a subnet with no other host and addresses that are not local network', () => {
  expect(
    localNetworkTargets({
      en5: [v4('10.0.0.5', '255.255.255.255')],
      en6: [v4('8.8.4.4', '255.255.255.0')],
      en7: [v4('172.32.0.9', '255.255.255.0')],
    })
  ).toEqual([]);
  expect(localNetworkTargets({ en8: [v4('10.1.2.1', '255.255.255.0')] })).toEqual([
    { interfaceName: 'en8', address: '10.1.2.2' },
  ]);
});
