#!/usr/bin/env python3
"""Verify the deployed HTTPS service, feed and optional full artifact downloads."""
import argparse
import hashlib
import json
import urllib.request
from urllib.parse import urlsplit
from publish import public_base


def request(url, method='GET'):
    public_base(url)  # Require HTTPS and reject credentials/fragments.
    response = urllib.request.urlopen(urllib.request.Request(url, method=method,
                                     headers={'Cache-Control': 'no-cache', 'User-Agent': 'CView-Server-Check'}), timeout=30)
    if urlsplit(response.geturl()).scheme != 'https':
        response.close()
        raise ValueError('Server redirected to an insecure URL')
    return response


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-url', required=True)
    parser.add_argument('--download', action='store_true', help='Download each artifact and verify SHA-256')
    args = parser.parse_args()
    try:
        base = public_base(args.base_url)
        with request(base + '/health.json') as response:
            if json.load(response).get('status') != 'ok':
                raise ValueError('Unexpected health response')
        with request(base + '/releases/latest.json') as response:
            release = json.load(response)
        if not release.get('tag_name') or not release.get('assets'):
            raise ValueError('No valid release has been published')
        with request(release['html_url']) as response:
            if response.status != 200:
                raise ValueError('Release page is unavailable')
        for asset in release['assets']:
            with request(asset['browser_download_url'], 'HEAD') as response:
                if int(response.headers['Content-Length']) != asset['size']:
                    raise ValueError('Artifact size mismatch: ' + asset['name'])
            if args.download:
                digest = hashlib.sha256()
                with request(asset['browser_download_url']) as response:
                    for chunk in iter(lambda: response.read(1024 * 1024), b''):
                        digest.update(chunk)
                if digest.hexdigest() != asset['sha256']:
                    raise ValueError('Artifact SHA-256 mismatch: ' + asset['name'])
        print(f'Verified {release["tag_name"]}: HTTPS, health, release page, {len(release["assets"])} artifacts'
              + (' and SHA-256 downloads.' if args.download else '.'))
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f'Server verification failed: {error}\n')


if __name__ == '__main__':
    main()
