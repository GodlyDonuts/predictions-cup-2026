"""Read one unauthenticated public leaderboard document; never an account API.

The marked-PnL anchor is a dated incomplete-field assumption, not opposing cash,
terminal wealth, a calibrated forecast, or authority to trade.
"""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import re
import urllib.request


def parse_leader(raw, tournament_slug):
    text = raw.decode('utf-8')
    parts = [json.loads(m.group(1)) for m in re.finditer(
        r'self\.__next_f\.push\(\[1,("(?:\\.|[^"\\])*")\]\)', text)]
    text = ''.join(parts)
    for _ in range(3):
        text = text.replace(r'\"', '"')
    assert text.count('"entries":[{"rank":1,') == 1, 'AMBIGUOUS_PUBLIC_FIELD'
    start = text.index('{"rank":1,', text.index('"standingsStatus":"ready"'))
    decoder = json.JSONDecoder(parse_float=Decimal)
    leader, end = decoder.raw_decode(text, start)
    at = text.index('"queryKey":', end) + len('"queryKey":')
    key, _ = decoder.raw_decode(text, at)
    assert key == ['tournament-leaderboard-tab', 'tournament', tournament_slug,
                   'all', None, '', 'pnl'], 'PUBLIC_TOURNAMENT_OR_RANKING_MISMATCH'
    assert leader['rank'] == 1 and isinstance(leader['username'], str)
    assert re.fullmatch(r'[a-zA-Z0-9_-]{1,80}', leader['username'])
    assert type(leader['pnl']) in (int, Decimal) and Decimal(leader['pnl']).is_finite()
    assert type(leader['tradesCount']) is int and leader['tradesCount'] >= 0
    assert type(leader['volume']) in (int, Decimal) and Decimal(leader['volume']).is_finite()
    assert Decimal(leader['volume']) >= 0
    return leader, key


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output-directory', type=Path, required=True)
    p.add_argument('--tournament-slug', required=True)
    p.add_argument('--initial-allotment-assumption', type=Decimal, required=True)
    a = p.parse_args()
    assert a.initial_allotment_assumption > 0
    url = 'https://sig.thesuper.market/leaderboards'
    started = datetime.now(timezone.utc).isoformat()
    req = urllib.request.Request(url, headers={'User-Agent': 'Moloch public field observation',
                                               'Accept': 'text/html'})
    with urllib.request.urlopen(req, timeout=20) as response:
        assert response.status == 200 and response.url == url
        raw = response.read(8 * 1024 * 1024 + 1)
        assert len(raw) <= 8 * 1024 * 1024
        headers = {k: response.headers.get(k) for k in ('Date', 'Age', 'Cache-Control')}
    received = datetime.now(timezone.utc).isoformat()
    leader, key = parse_leader(raw, a.tournament_slug)
    d = {'schema': 'tartarus_public_leader_observation_v1', 'url': url,
         'requestStartedAt': started, 'receivedAt': received, 'status': 200,
         'rawBytes': len(raw), 'rawSha256': hashlib.sha256(raw).hexdigest(),
         'unauthenticatedPublicPage': True, 'accountApiRequests': 0,
         'tournamentSlug': a.tournament_slug, 'originalQueryKey': key,
         'limitedPublicCacheHeaders': headers, 'sourcePublicationTime': None,
         'publicRank': 1, 'publicUsername': leader['username'],
         'publicPnl': str(leader['pnl']), 'publicTradesCount': leader['tradesCount'],
         'publicVolume': str(leader['volume']), 'publicRoiField': str(leader['roi']),
         'publicRoiDefinitionNotInferred': True,
         'initialAllotmentAssumption': format(a.initial_allotment_assumption, '.6f'),
         'derivedFlatMarkedWealthAnchor': format(a.initial_allotment_assumption + leader['pnl'], '.6f'),
         'anchorIsCash': False, 'anchorIsTerminalWealth': False,
         'unknownOpposingPositions': True,
         'projectionAssumption': 'Assumed initial allotment plus dated public marked PnL. Holding this level flat is an incomplete-field hypothesis, not a rival forecast.',
         'executionAuthority': False}
    a.output_directory.mkdir(parents=True, exist_ok=False)
    (a.output_directory / 'leaderboard.original.html').write_bytes(raw)
    (a.output_directory / 'observation.json').write_text(json.dumps(d, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(d))


if __name__ == '__main__':
    main()
