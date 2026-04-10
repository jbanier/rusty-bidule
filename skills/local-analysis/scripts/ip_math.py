#!/usr/bin/env python3
import argparse
import ipaddress

from common import add_common_text_arg, json_dump, read_text_argument


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    args = parser.parse_args()
    text = read_text_argument(args).strip()

    if "/" in text:
        network = ipaddress.ip_network(text, strict=False)
        payload = {
            "broadcast_address": str(network.broadcast_address) if network.version == 4 else None,
            "cidr": str(network),
            "is_private": network.is_private,
            "netmask": str(network.netmask),
            "network_address": str(network.network_address),
            "num_addresses": network.num_addresses,
            "prefixlen": network.prefixlen,
            "reverse_pointer": network.network_address.reverse_pointer,
            "version": network.version,
        }
    else:
        ip = ipaddress.ip_address(text)
        payload = {
            "compressed": ip.compressed,
            "exploded": getattr(ip, "exploded", ip.compressed),
            "is_global": ip.is_global,
            "is_loopback": ip.is_loopback,
            "is_multicast": ip.is_multicast,
            "is_private": ip.is_private,
            "is_reserved": ip.is_reserved,
            "normalized": str(ip),
            "reverse_pointer": ip.reverse_pointer,
            "version": ip.version,
        }
    json_dump(payload)


if __name__ == "__main__":
    main()
