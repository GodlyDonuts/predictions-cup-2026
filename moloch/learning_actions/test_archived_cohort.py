import hashlib,json,tempfile,unittest
from pathlib import Path
from .compile_archived_cohort import coverage,verify_snapshot,compile_cohort


class ArchivedCohortTests(unittest.TestCase):
    def rows(self):
        return [dict(calendar_role=role,venue=venue,kind='book'if venue=='SIG'else'reference',available_at_ms=at)
                for role,ats in [('fit',[0,30,60]),('search',[100,130,160])]
                for venue in('SIG','Kalshi','Polymarket')for at in ats]

    def test_coverage_requires_every_venue_and_both_boundaries_without_imputation(self):
        roles={'fit':(0,90),'search':(100,190)};rows=self.rows()
        proof=coverage(rows,roles,30);self.assertEqual(len(proof),6)
        self.assertEqual(proof['search/Polymarket']['maximumObservedGapMs'],30)
        for role,venue in [('fit','SIG'),('fit','Polymarket'),('search','Kalshi')]:
            subset=[r for r in rows if not(r['calendar_role']==role and r['venue']==venue and r['available_at_ms']in(30,130))]
            with self.assertRaisesRegex(ValueError,'COVERAGE_GAP')as caught:coverage(subset,roles,30)
            self.assertEqual(caught.exception.coverage_gap['duration_ms'],60)
            self.assertEqual(caught.exception.coverage_gap['role'],role)
            self.assertEqual(caught.exception.coverage_gap['venue'],venue)
        with self.assertRaisesRegex(ValueError,'NO_VALID_SOURCE'):coverage([r for r in rows if r['venue']!='Polymarket'],roles,30)

    def test_exact_role_end_is_excluded_and_invalid_reference_is_not_valid_coverage(self):
        rows=self.rows();roles={'fit':(0,90),'search':(100,190)}
        rows.append(dict(calendar_role='fit',venue='SIG',kind='book',available_at_ms=90))
        with self.assertRaisesRegex(ValueError,'OUTSIDE_FROZEN'):coverage(rows,roles,30)
        rows=self.rows()
        for row in rows:
            if row['venue']=='Polymarket':row['kind']='reference_gap'
        with self.assertRaisesRegex(ValueError,'NO_VALID_SOURCE'):coverage(rows,roles,30)

    def test_archive_bytes_size_and_regular_file_are_verified(self):
        with tempfile.TemporaryDirectory()as directory:
            p=Path(directory)/'original.snapshot';p.write_bytes(b'original')
            pin={'path':str(p),'source_size':8,'sha256':hashlib.sha256(b'original').hexdigest()}
            self.assertEqual(verify_snapshot(pin)[0],p)
            p.write_bytes(b'changed!')
            with self.assertRaisesRegex(ValueError,'BYTES_CHANGED'):verify_snapshot(pin)
            p.write_bytes(b'short')
            with self.assertRaisesRegex(ValueError,'SIZE_CHANGED'):verify_snapshot(pin)

    def test_wrong_tournament_or_protected_calendar_fails_before_creating_output(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);p=root/'protocol.json';out=root/'output'
            protocol={'classification':'retrospective_development_only_not_four_day_evolution_or_independent_test',
                'independentTestIncluded':False,'productionAuthority':False,'tournamentId':'correct',
                'calendarRolesUtc':{'fit':['2026-10-19T11:30:00Z','2026-10-19T13:15:00Z'],
                                    'search':['2026-10-19T13:20:00Z','2026-10-19T14:10:00Z']},
                'protectedCalendarsExcludedUtc':[['2026-10-19T12:00:00Z','2026-10-19T12:30:00Z']]}
            p.write_text(json.dumps(protocol))
            with self.assertRaisesRegex(ValueError,'FROZEN_TOURNAMENT'):
                compile_cohort(root,root/'missing',p,root/'missing',root/'missing',out,'wrong')
            with self.assertRaisesRegex(ValueError,'PROTECTED_CALENDAR'):
                compile_cohort(root,root/'missing',p,root/'missing',root/'missing',out,'correct')
            self.assertFalse(out.exists())
