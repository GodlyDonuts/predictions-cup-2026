from decimal import Decimal as D
from fractions import Fraction as F
import json
from pathlib import Path
import unittest
import verify as v


class FiniteSourceChecks(unittest.TestCase):
    def test_all_fifteen_ceilings_against_independent_rationals(self):
        for _, _, _, _, p in v.SPECS:
            for r in v.stress(p, '.5'):
                value = (F(1) - F(r['exceptionAllowanceAssumption'])) * F(p)
                ceiling = ((value - F(1, 100)) // F(1, 200)) * F(1, 200)
                self.assertEqual(F(r['tickFlooredResearchCeiling']), ceiling)
                self.assertFalse(r['calibratedProbabilityBound'])

    def test_stress_boundary_is_floored_and_not_rounded_up(self):
        self.assertEqual(v.stress('.84', '.745')[1]['tickFlooredResearchCeiling'], '0.745')
        self.assertTrue(v.stress('.84', '.745')[1]['passesHistoricalPriceArithmeticOnly'])
        self.assertFalse(v.stress('.84', '.750')[1]['passesHistoricalPriceArithmeticOnly'])
        with self.assertRaises(ValueError):
            v.stress('1.001', '.5')

    def test_changed_raw_source_fails(self):
        from hashlib import sha256
        pin = {'sha256': sha256(b'original').hexdigest(), 'bytes': 8}
        v.check(b'original', pin)
        with self.assertRaises(ValueError):
            v.check(b'changed!', pin)

    def test_kansas_general_scope_and_contact_columns(self):
        rows = [
            ['Republican', 'Ty Masterson / Jeffrey Klemp', 'Governor', '2026 General', 'PRIVATE CONTACT'],
            ['Democratic', 'Cindy Holscher / KC Ohaebosim', 'Governor', '2026 General', 'PRIVATE CONTACT'],
        ]
        html = '<table>' + ''.join('<tr>' + ''.join('<td>'+c+'</td>' for c in row) + '</tr>' for row in rows) + '</table>'
        result = v.kansas(html.encode())
        self.assertEqual(len(result[0]), 4)
        self.assertNotIn('PRIVATE', str(result))
        with self.assertRaises(ValueError):
            v.kansas(html.replace('2026 General', '2026 Primary').encode())

    def test_texas_primary_or_wrong_office_is_not_nominee_proof(self):
        certification = 'final ballot certification'
        page = '2026 NOVEMBER GENERAL ELECTION November 03, 2026 GOVERNOR GREG ABBOTT REP GINA HINOJOSA DEM PAT DIXON LIB LIEUTENANT GOVERNOR x'
        self.assertEqual(len(v.texas(certification, page)), 3)
        for changed in [page.replace('GENERAL', 'PRIMARY'), page.replace('GOVERNOR GREG', 'SENATOR GREG'), page.replace('ABBOTT REP', 'ABBOTT DEM')]:
            with self.assertRaises(ValueError):
                v.texas(certification, changed)

    def test_alaska_preserves_writeins_and_governor_party(self):
        names = ['Bronson, Dave (Registered Republican)', 'Certified Write-In DeWitt, Meda (Nonpartisan)',
                 'Kreiss-Tomkins, Jonathan S. JKT (Registered Democrat / Nonpartisan)',
                 'Certified Write-In Parkin, James W. (Registered Republican / Registered Democrat)',
                 'Taylor, Treg (Registered Republican)', 'Wilson, Bernadette M. (Registered Republican)']
        html = '2026 General Election GOVERNOR / LIEUTENANT GOVERNOR<table>' + ''.join('<tr><td>'+n+' (Certified)</td><td>PRIVATE CONTACT</td></tr>' for n in names) + '</table>'
        result = v.alaska(html.encode())
        self.assertEqual(len(result), 6)
        self.assertIn('Republican / Registered Democrat', result[3])
        self.assertNotIn('PRIVATE CONTACT', str(result))
        with self.assertRaises(ValueError):
            v.alaska(html.replace('Registered Democrat / Nonpartisan', 'Registered Republican / Nonpartisan').encode())

    def test_dossier_does_not_impute_censored_or_omitted_probabilities(self):
        d = json.loads((Path(__file__).parent/'dossier.json').read_bytes())
        ak = next(x for x in d['candidates'] if x['state']=='AK')
        t = next(x for x in ak['allOriginalVisibleForecastCandidates'] if x['name']=='Treg Taylor')
        self.assertIsNone(t['candidateWinModelPoint'])
        self.assertEqual(D(t['strictUpperIfCensored']), D('.001'))
        self.assertFalse(d['executionAuthority'])
        fl = next(x for x in d['candidates'] if x['state']=='FL')
        self.assertFalse(fl['officialNominee']['originalGeneralElectionNomineeSourceBodyVerified'])
        self.assertEqual(fl['reviewStatus'], 'partial_original_nominee_source_missing')
        self.assertTrue(all(not x['rawOriginalBodyVerified'] for x in fl['officialNominee']['primaryRepresentationsNotOriginalBodies']))


if __name__ == '__main__':
    unittest.main()
