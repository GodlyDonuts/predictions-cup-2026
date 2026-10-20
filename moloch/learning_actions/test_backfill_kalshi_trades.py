import io,json,tempfile,unittest,urllib.error
from pathlib import Path
from .backfill_kalshi_trades import suffix,validate,recover,digest,fetch


def receipt(path,cursor='',*,clock='2026-10-01T00:00:01Z'):
    body={'cursor':cursor,'trades':[{'ticker':'CONTROLH-2026-D','trade_id':'original-id',
        'created_time':clock,'count_fp':'2.50','yes_price_dollars':'0.56','no_price_dollars':'0.44'}]}
    text=json.dumps(body)
    return {'source':'kalshi_official_v2','status':200,'path':'/trade-api/v2'+path,
        'requestStartedAt':'2026-10-20T00:00:00Z','receivedAt':'2026-10-20T00:00:01Z',
        'body':body,'bodyText':text,'bodySha256':digest(text.encode())}


class BackfillTests(unittest.TestCase):
    lo=1790812800;hi=1790812802;ticker='CONTROLH-2026-D'

    def test_original_event_and_present_delivery_remain_separate(self):
        path=suffix(self.ticker,self.lo,self.hi);r=receipt(path);d=validate(r,path,self.ticker,self.lo,self.hi)
        self.assertEqual(d['firstEventMs'],(self.lo+1)*1000)
        self.assertEqual(d['receivedAt'],'2026-10-20T00:00:01Z')
        self.assertEqual(r['body']['trades'][0]['count_fp'],'2.50')

    def test_boundary_is_retained_but_not_admitted_into_prior_split(self):
        path=suffix(self.ticker,self.lo,self.hi);d=validate(receipt(path,clock='2026-10-01T00:00:02Z'),path,self.ticker,self.lo,self.hi)
        self.assertEqual(d['records'],1);self.assertEqual(d['insideHalfOpenRange'],0)
        with self.assertRaises(ValueError):validate(receipt(path,clock='2026-10-01T00:00:03Z'),path,self.ticker,self.lo,self.hi)

    def test_wrong_body_identity_clock_or_path_rejected(self):
        path=suffix(self.ticker,self.lo,self.hi)
        for mutation in [lambda r:r.update(bodySha256='0'*64),lambda r:r['body'].update(cursor='forged'),
                         lambda r:r.update(path='/trade-api/v2/portfolio'),
                         lambda r:r.update(receivedAt='2026-09-01T00:00:00Z')]:
            r=receipt(path);mutation(r)
            with self.assertRaises(ValueError):validate(r,path,self.ticker,self.lo,self.hi)
        with self.assertRaises(ValueError):suffix('../portfolio',self.lo,self.hi)

    def test_resume_validates_existing_pages_without_reissuing_them(self):
        plan={'tickers':[self.ticker],'startTs':self.lo,'endTs':self.hi};calls=[]
        def get(path):
            calls.append(path);return receipt(path,'' if 'cursor='in path else 'next')
        with tempfile.TemporaryDirectory()as root:
            recover(root,plan,fetcher=get,maximum_pages=1)
            self.assertEqual(json.loads((Path(root)/'status.json').read_text())['state'],'bounded_incomplete')
            recover(root,plan,fetcher=get)
            self.assertEqual(len(calls),2)
            state=json.loads((Path(root)/'status.json').read_text());self.assertEqual(state['completedTickers'],1)
            self.assertFalse(state['trainingAdmitted']);self.assertFalse(state['originalHistoricalDeliveryVerified'])
            with self.assertRaises(ValueError):recover(root,{**plan,'endTs':self.hi+1},fetcher=get)

    def test_repeated_cursor_and_corrupt_existing_page_fail_closed(self):
        plan={'tickers':[self.ticker],'startTs':self.lo,'endTs':self.hi}
        with tempfile.TemporaryDirectory()as root:
            with self.assertRaises(ValueError):recover(root,plan,fetcher=lambda p:receipt(p,'repeat'))
            p=next((Path(root)/'pages').glob('*.json'));r=json.loads(p.read_text());r['bodySha256']='0'*64;p.write_text(json.dumps(r))
            with self.assertRaises(ValueError):recover(root,plan,fetcher=lambda p:self.fail('must verify saved receipt first'))

    def test_wrapped_authorization_failure_is_terminal(self):
        from unittest.mock import patch,Mock
        opener=Mock();opener.open.side_effect=urllib.error.HTTPError('http://127.0.0.1',503,'wrapped',{},io.BytesIO(b'{"code":401}'))
        with patch('urllib.request.build_opener',return_value=opener):
            with self.assertRaises(PermissionError):fetch(suffix(self.ticker,self.lo,self.hi))
        self.assertEqual(opener.open.call_count,1)


if __name__=='__main__':unittest.main()
