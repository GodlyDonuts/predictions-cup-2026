from pathlib import Path
import tempfile
import unittest
from xml.sax.saxutils import escape
import zipfile

import validate as v


def nv(heading="GOVERNOR",party="REP",context="2026 General Election",duplicate=False):
    target=f"<tr><td>Lombardo, Joseph</td><td>{party}</td></tr>"
    return (f"<h1>{context}</h1><table><tr><th>{heading}</th></tr><tr><td>Ford, Aaron D.</td><td>DEM</td></tr>"
            f"<tr><td>Ford, Danielle</td><td>NPP</td></tr>{target}{target if duplicate else ''}<tr><td>None of these candidates</td><td>--</td></tr></table>").encode()


def workbook(path,party="REPUBLICAN",office="GOVERNOR",wrong_header=False,duplicate=False):
    ns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
    def row(n,values):
        return '<row r="'+str(n)+'">'+''.join(f'<c r="{col}{n}" t="inlineStr"><is><t>{escape(value)}</t></is></c>' for col,value in values.items())+'</row>'
    h={"A":"Contest","C":"Name On Ballot","E":"Party" if not wrong_header else "Address"}
    data={"A":office,"C":"PHIL SCOTT","E":party,"F":"PRIVATE CONTACT MUST NOT APPEAR"}
    content=row(1,h)+row(12,data)+(row(13,data) if duplicate else '')
    with zipfile.ZipFile(path,"w") as z:
        z.writestr("xl/sharedStrings.xml",f'<sst xmlns="{ns}"/>')
        z.writestr("xl/worksheets/sheet1.xml",f'<worksheet xmlns="{ns}"><sheetData>{content}</sheetData></worksheet>')
        z.writestr("xl/worksheets/sheet2.xml",f'<worksheet xmlns="{ns}"><sheetData>{row(1,{"A":"Last Updated: 9/23/2026"})}</sheetData></worksheet>')
        z.writestr("docProps/core.xml",'<core xmlns:d="http://purl.org/dc/terms/"><d:modified>2026-09-23T19:19:12Z</d:modified></core>')


class NomineeTests(unittest.TestCase):
    def test_nevada_exact_general_office_and_party_preserve_independent_and_none(self):
        rows=v.nevada_rows(nv())
        self.assertEqual(rows[2],{"nameOnBallot":"LOMBARDO, JOSEPH","partyOnBallot":"REP"})
        self.assertEqual(rows[1]["partyOnBallot"],"NPP")
        self.assertEqual(rows[-1]["nameOnBallot"],"NONE OF THESE CANDIDATES")

    def test_primary_wrong_office_party_and_duplicate_cannot_certify(self):
        for raw in [nv(context="2026 Primary Election"),nv(heading="LIEUTENANT GOVERNOR"),nv(party="NPP"),nv(duplicate=True)]:
            with self.assertRaises(ValueError):v.nevada_rows(raw)

    def test_access_challenge_is_not_successful_candidate_html(self):
        with self.assertRaises(ValueError):v.nevada_rows(b'<h1>Additional security check required</h1>')

    def test_workbook_extracts_only_office_name_party_and_original_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"official.xlsx";workbook(path);got=v.workbook_rows(path)
            self.assertEqual(got["governorRows"],[{"row":12,"office":"GOVERNOR","nameOnBallot":"PHIL SCOTT","partyOnBallot":"REPUBLICAN"}])
            self.assertNotIn("PRIVATE",str(got))
            self.assertEqual(got["selectionCriteria"],"Last Updated: 9/23/2026")
            self.assertEqual(got["originalDocumentModified"],"2026-09-23T19:19:12Z")

    def test_workbook_wrong_party_office_duplicate_or_header_refuses(self):
        with tempfile.TemporaryDirectory() as tmp:
            for kw in [{"party":"DEM/PROG"},{"office":"LIEUTENANT GOVERNOR"},{"duplicate":True},{"wrong_header":True}]:
                path=Path(tmp)/"official.xlsx";workbook(path,**kw)
                with self.assertRaises(ValueError):v.workbook_rows(path)

    def test_spa_shell_script_and_primary_heading_cannot_be_general_ballot(self):
        for raw in [b'<app-root></app-root>',b'<script>11/03/2026 STATE GENERAL ELECTION U.S. Senator Michele Tafoya REPUBLICAN</script>',b'08/11/2026 STATE PRIMARY ELECTION U.S. Senator Michele Tafoya REPUBLICAN']:
            self.assertFalse(v.original_ballot_row_present(raw))

    def test_literal_rendered_general_row_is_distinct_from_unsupported_shell(self):
        self.assertTrue(v.original_ballot_row_present(b'<h2>11/03/2026 STATE GENERAL ELECTION</h2><table><tr><td>U.S. Senator</td><td>Michele Tafoya</td><td>REPUBLICAN</td></tr></table>'))


if __name__=="__main__":
    unittest.main()
