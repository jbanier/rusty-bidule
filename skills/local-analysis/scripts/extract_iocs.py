#!/usr/bin/env python3
import argparse
import re

from common import add_common_text_arg, read_text_argument, json_dump


IPV4 = re.compile(r"\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b")
URL = re.compile(r"\b(?:https?|ftp)://[^\s<>()\"']+")
EMAIL = re.compile(r"\b[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[A-Za-z]{2,}\b")
DOMAIN = re.compile(r"\b(?=.{1,253}\b)(?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?\.)+[A-Za-z]{2,63}\b")
MD5 = re.compile(r"\b[a-fA-F0-9]{32}\b")
SHA1 = re.compile(r"\b[a-fA-F0-9]{40}\b")
SHA256 = re.compile(r"\b[a-fA-F0-9]{64}\b")
CVE = re.compile(r"\bCVE-\d{4}-\d{4,7}\b", re.IGNORECASE)
ATTACK = re.compile(r"\bT\d{4}(?:\.\d{3})?\b", re.IGNORECASE)


def uniq(items: list[str]) -> list[str]:
    return sorted({item for item in items if item})


def main() -> None:
    parser = argparse.ArgumentParser()
    add_common_text_arg(parser)
    args = parser.parse_args()
    text = read_text_argument(args)

    urls = uniq(URL.findall(text))
    emails = uniq(EMAIL.findall(text))
    ipv4 = uniq(IPV4.findall(text))
    domains = uniq(
        domain
        for domain in DOMAIN.findall(text)
        if domain not in {email.split("@", 1)[1] for email in emails}
        and not any(domain in url for url in urls)
    )

    payload = {
        "counts": {
            "attack_ids": len(uniq([value.upper() for value in ATTACK.findall(text)])),
            "cves": len(uniq([value.upper() for value in CVE.findall(text)])),
            "domains": len(domains),
            "emails": len(emails),
            "ipv4": len(ipv4),
            "md5": len(uniq(MD5.findall(text))),
            "sha1": len(uniq(SHA1.findall(text))),
            "sha256": len(uniq(SHA256.findall(text))),
            "urls": len(urls),
        },
        "iocs": {
            "attack_ids": uniq([value.upper() for value in ATTACK.findall(text)]),
            "cves": uniq([value.upper() for value in CVE.findall(text)]),
            "domains": domains,
            "emails": emails,
            "ipv4": ipv4,
            "md5": uniq(MD5.findall(text)),
            "sha1": uniq(SHA1.findall(text)),
            "sha256": uniq(SHA256.findall(text)),
            "urls": urls,
        },
    }
    json_dump(payload)


if __name__ == "__main__":
    main()
