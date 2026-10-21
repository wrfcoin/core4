import contextlib
import importlib.util
import io
import re
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("shadow_report", Path(__file__).with_name("shadow_report.py"))
sr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sr)

RADIUS = 10.0


def line(commit, station, fixed_ref, fixed_tier, fixed_conf, resolved, distance, ps_tier, ps_conf, would, notes='[]'):
    return (f'[2026-10-03T14:00:00Z INFO oracle_attester] [attester] SHADOW reference: commit_id={commit} station_id={station} '
            f'fixed_ref={fixed_ref} fixed_tier={fixed_tier} fixed_confidence={fixed_conf:.3f} resolved_ref={resolved} '
            f'distance_km={distance} per_station_tier={ps_tier} per_station_confidence={ps_conf:.3f} would_change={would} '
            f'notes={notes}')


SUMMARY = ('[x INFO oracle_attester] [attester] SHADOW summary: would_change=2 distinct_reference_stations=2 '
           'nws_lookups_this_cycle=5')
# Two synthetic stations with DISTINCT per-station references, one far station declared physics-only at the cap.
LOG = '\n'.join([
    'unrelated line that is not a shadow line',
    line('c1', 'syn-station-a', 'KDLH', 'good', 0.95, 'KDLH', '3.4', 'good', 0.95, 'false'),
    line('c2', 'syn-station-b', 'KDLH', 'good', 0.95, 'KMSP', '4.1', 'degraded', 0.70, 'true'),
    line('c3', 'syn-station-far', 'KDLH', 'good', 0.95, 'none', 'n/a', 'PHYSICS-ONLY', 0.500, 'true',
         '["no NWS station within 10 km"]'),
    SUMMARY,
])


def report(log=LOG, radius=RADIUS, **kw):
    records, summaries = sr.parse_log(log)
    return sr.build_report(records, summaries, radius, **kw)


class ShadowReportTests(unittest.TestCase):
    def test_two_stations_use_distinct_reference_stations(self):
        text, problems = report()
        self.assertEqual(problems, [])
        self.assertIn('2 distinct (KDLH, KMSP)', text)
        self.assertIn('syn-station-a', text)
        self.assertIn('syn-station-b', text)

    def test_out_of_radius_is_declared_physics_only_at_the_cap(self):
        text, problems = report()
        self.assertEqual(problems, [])
        row = next(l for l in text.splitlines() if l.startswith('syn-station-far'))
        self.assertIn('OUT_OF_RADIUS', row)
        self.assertIn('PHYSICS-ONLY/0.500', row)

    def test_out_of_radius_with_a_scored_tier_is_a_violation(self):
        bad = LOG.replace('resolved_ref=none distance_km=n/a per_station_tier=PHYSICS-ONLY per_station_confidence=0.500',
                          'resolved_ref=none distance_km=n/a per_station_tier=good per_station_confidence=0.950')
        text, problems = report(bad)
        self.assertEqual(len(problems), 1)
        self.assertIn('not PHYSICS-ONLY at the 0.5 cap', problems[0])
        self.assertIn('RESULT                   : VIOLATIONS', text)

    def test_physics_only_at_the_wrong_cap_is_a_violation(self):
        bad = LOG.replace('per_station_confidence=0.500', 'per_station_confidence=0.800')
        _, problems = report(bad)
        self.assertEqual(len(problems), 1)

    def test_beyond_radius_with_a_resolved_station_must_also_be_physics_only(self):
        far = line('c9', 'syn-far-resolved', 'KDLH', 'good', 0.95, 'KABC', '12.5', 'good', 0.95, 'true')
        _, problems = report(far)
        self.assertEqual(len(problems), 1)

    def test_radius_and_uncertainty_are_named_separately(self):
        text, _ = report()
        radius = next(l for l in text.splitlines() if l.startswith('configured radius'))
        unc = next(l for l in text.splitlines() if l.startswith('coarse-location uncertainty'))
        self.assertIn('10 km', radius)
        self.assertNotIn('0.8', radius)
        self.assertIn('0.8 km', unc)
        self.assertIn('0.01 deg', unc)
        self.assertIn('separate from the radius', unc)

    def test_borderline_inside_the_radius_is_marked_not_widened(self):
        inside = line('c7', 'syn-edge', 'KDLH', 'good', 0.95, 'KABC', '9.5', 'good', 0.95, 'false')
        text, problems = report(inside)
        self.assertEqual(problems, [])
        row = next(l for l in text.splitlines() if l.startswith('syn-edge'))
        self.assertIn('BORDERLINE', row)
        self.assertIn('BORDERLINE=1', text)

    def test_just_over_the_radius_is_out_even_within_the_uncertainty(self):
        over = line('c8', 'syn-edge', 'KDLH', 'good', 0.95, 'KABC', '10.4', 'good', 0.95, 'true')
        _, problems = report(over)
        self.assertEqual(len(problems), 1)

    def test_summary_and_would_change_are_reported(self):
        text, _ = report()
        self.assertIn('would_change=2 distinct_reference_stations=2 nws_lookups_this_cycle=5', text)
        self.assertIn('would change a verdict   : 2 of 3', text)

    def test_fixed_reference_is_the_submitted_one_and_is_shown(self):
        text, _ = report()
        self.assertIn('fixed reference(s)       : KDLH', text)
        self.assertIn('the submitted verdict is the FIXED one', text)

    def test_byte_identity_is_honestly_not_claimed(self):
        text, _ = report()
        self.assertIn('NOT EVIDENCED HERE', text)
        self.assertIn('byte-identity of the submitted fixed-mode verdict', text)

    def test_no_coordinates_in_the_report(self):
        text, _ = report()
        self.assertNotRegex(text, r'-?\d{1,3}\.\d{4,}')
        self.assertNotIn('latitude', text.lower())

    def test_only_shadow_lines_are_read_and_a_malformed_shadow_line_refuses(self):
        with self.assertRaises(sr.ReportError):
            sr.parse_log('x SHADOW reference: commit_id=c station_id=s but the rest is missing')

    def test_bounded_inputs(self):
        many = '\n'.join(line(f'c{i}', f's{i}', 'KDLH', 'good', 0.95, 'KDLH', '3.4', 'good', 0.95, 'false') for i in range(12))
        with self.assertRaises(sr.ReportError):
            sr.parse_log(many, max_lines=10)
        with self.assertRaises(sr.ReportError):
            sr.parse_log('SHADOW reference: ' + 'x' * (sr.MAX_LINE_CHARS + 1))
        text, _ = report(many)
        self.assertEqual(sum(1 for l in text.splitlines() if __import__('re').match(r's\d', l)), 12)  # under the row print bound
        big = '\n'.join(line(f'c{i}', f's{i:03d}', 'KDLH', 'good', 0.95, 'KDLH', '3.4', 'good', 0.95, 'false') for i in range(80))
        text, _ = report(big)
        self.assertIn('30 more row(s) not printed', text)
        self.assertEqual(sum(1 for l in text.splitlines() if __import__('re').match(r's\d{3}', l)), sr.MAX_ROWS_PRINTED)

    def test_inputs_are_validated(self):
        records, summaries = sr.parse_log(LOG)
        for radius in (0, -1, None, 'ten'):
            with self.assertRaises(sr.ReportError):
                sr.build_report(records, summaries, radius)
        with self.assertRaises(sr.ReportError):
            sr.build_report([], [], RADIUS)

    def test_cli_exit_codes(self):
        with tempfile.TemporaryDirectory() as d:
            ok = Path(d) / 'ok.log'
            ok.write_text(LOG)
            bad = Path(d) / 'bad.log'
            bad.write_text(LOG.replace('per_station_confidence=0.500', 'per_station_confidence=0.900'))
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(sr.main(['--max-reference-km', '10', str(ok)]), 0)
                self.assertEqual(sr.main(['--max-reference-km', '10', str(bad)]), 1)
                self.assertEqual(sr.main(['--max-reference-km', '0', str(ok)]), 2)
                self.assertEqual(sr.main(['--max-reference-km', '10', str(Path(d) / 'missing.log')]), 2)
        self.assertIn('RESULT                   : CONSISTENT', out.getvalue())

    # ---- SOL-R review of the first head: real daemon line formats and non-loosenable checks -------------------

    ATTESTER = Path(__file__).resolve().parents[3] / 'native-chain' / 'src' / 'bin' / 'oracle_attester.rs'

    def test_synthetic_record_formats_match_the_attester_source(self):
        src = self.ATTESTER.read_text()
        fmt = re.search(r'"\[attester\] (SHADOW reference: .*?)",\s*entry\.commit_id', src, re.S).group(1)
        fmt = re.sub(r'\\\s*', ' ', fmt)  # Rust string continuations
        fields = re.findall(r'(\w+)=\{', fmt)
        self.assertEqual(fields, ['commit_id', 'station_id', 'fixed_ref', 'fixed_tier', 'fixed_confidence', 'resolved_ref',
                                  'distance_km', 'per_station_tier', 'per_station_confidence', 'would_change', 'notes'])
        sample = line('c1', 's1', 'KDLH', 'good', 0.95, 'KDLH', '3.4', 'good', 0.95, 'false')
        self.assertEqual(re.findall(r'(\w+)=', sample.split('SHADOW reference: ')[1]), fields)
        summary = re.search(r'"\[attester\] (SHADOW summary: .*?)",', src, re.S).group(1)
        summary = re.sub(r'\\\s*', ' ', summary)
        self.assertEqual(re.findall(r'(\w+)=', summary), ['would_change', 'distinct_reference_stations', 'nws_lookups_this_cycle'])

    def test_the_real_startup_warning_is_skipped_not_refused(self):
        # Copied from oracle_attester.rs (reference mode warning, shadow variant), including its "SHADOW:" suffix.
        startup = ('[2026-10-03T14:00:00Z WARN oracle_attester] [attester] reference mode shadow: per-commit nearest NWS station '
                   'within 10 km (lookup timeout 5s, at most 8 stations per cycle); fallback = configured station '
                   'Some("KDLH"), then PHYSICS-ONLY (#3240) \u2014 SHADOW: the submitted verdict is still the fixed one')
        self.assertIn('SHADOW: the submitted verdict is still the fixed one', self.ATTESTER.read_text())
        text, problems = report(startup + '\n' + LOG)
        self.assertEqual(problems, [])
        self.assertIn('commits compared         : 3', text)
        # A startup warning alone is "no records", a refusal of an EMPTY report, not a format failure.
        with self.assertRaisesRegex(sr.ReportError, 'no SHADOW reference lines'):
            report(startup)
        # A malformed RECORD is still refused.
        with self.assertRaises(sr.ReportError):
            sr.parse_log(startup + '\nSHADOW reference: commit_id=c station_id=s')

    def test_selected_reference_with_unknown_distance_is_unverified_not_out_of_radius(self):
        # The daemon's missing-registry-location fallback: configured station selected, distance_km=n/a, a scored verdict.
        fallback = line('c10', 'syn-no-location', 'KDLH', 'good', 0.95, 'KDLH', 'n/a', 'good', 0.95, 'false',
                        '["reference: station has no registry location on the pending surface \u2014 falling back to the configured station",'
                        ' "reference: using configured station KDLH (fallback)"]')
        text, problems = report(fallback)
        self.assertEqual(problems, [])
        row = next(l for l in text.splitlines() if l.startswith('syn-no-location'))
        self.assertIn('UNKNOWN_DIST', row)
        self.assertNotIn('OUT_OF_RADIUS', row)
        self.assertIn('unverified distance      : 1 commit(s)', text)
        self.assertIn('RESULT                   : CONSISTENT', text)

    def test_no_reference_and_known_outside_remain_subject_to_the_cap(self):
        none_scored = line('c11', 'syn-none', 'KDLH', 'good', 0.95, 'none', 'n/a', 'good', 0.95, 'true')
        far_scored = line('c12', 'syn-far', 'KDLH', 'good', 0.95, 'KABC', '25.0', 'good', 0.95, 'true')
        for bad in (none_scored, far_scored):
            _, problems = report(bad)
            self.assertEqual(len(problems), 1, bad)

    def test_the_physics_only_cap_cannot_be_loosened(self):
        loose = line('c13', 'syn-none', 'KDLH', 'good', 0.95, 'none', 'n/a', 'PHYSICS-ONLY', 0.800, 'true')
        _, problems = report(loose)
        self.assertEqual(len(problems), 1)  # 0.800 is not the 0.5 source cap
        with tempfile.TemporaryDirectory() as d:
            log = Path(d) / 'loose.log'
            log.write_text(loose)
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(sr.main(['--max-reference-km', '10', str(log)]), 1)
            for args in (['--physics-only-cap', '0.8'], ['--physics-only-cap=0.8']):
                with contextlib.redirect_stderr(err), self.assertRaises(SystemExit) as raised:
                    sr.main(['--max-reference-km', '10', *args, str(log)])
                self.assertEqual(raised.exception.code, 2)
        self.assertNotIn('physics_only_cap', sr.build_report.__code__.co_varnames)

    def test_the_radius_must_be_finite_and_positive(self):
        far_scored = line('c14', 'syn-far', 'KDLH', 'good', 0.95, 'KABC', '5000.0', 'good', 0.95, 'true')
        for radius in ('inf', '-inf', 'nan', '0', '-3', '1e999'):
            with tempfile.TemporaryDirectory() as d:
                log = Path(d) / 'r.log'
                log.write_text(far_scored)
                out = io.StringIO()
                with contextlib.redirect_stdout(out):
                    self.assertEqual(sr.main([f'--max-reference-km={radius}', str(log)]), 2, radius)
                self.assertTrue(out.getvalue().startswith('STOP:'), radius)
        records, summaries = sr.parse_log(far_scored)
        for radius in (float('inf'), float('nan'), True, 0, -1):
            with self.assertRaises(sr.ReportError):
                sr.build_report(records, summaries, radius)

    def test_the_whole_input_read_is_bounded(self):
        original = sr.MAX_INPUT_BYTES
        sr.MAX_INPUT_BYTES = 1000
        try:
            with tempfile.TemporaryDirectory() as d:
                big = Path(d) / 'big.log'
                big.write_text('daemon noise line that is not a record\n' * 100 + LOG)
                out = io.StringIO()
                with contextlib.redirect_stdout(out):
                    self.assertEqual(sr.main(['--max-reference-km', '10', str(big)]), 2)
                self.assertIn('unbounded read', out.getvalue())
                small = Path(d) / 'small.log'
                small.write_text(LOG)
                self.assertGreater(len(LOG.encode()), 0)
        finally:
            sr.MAX_INPUT_BYTES = original

    def test_the_tool_is_offline(self):
        src = Path(__file__).with_name('shadow_report.py').read_text()
        imports = set(__import__('re').findall(r'^(?:import|from)\s+(\w+)', src, __import__('re').M))
        self.assertLessEqual(imports, {'argparse', 'math', 're', 'sys'})


if __name__ == '__main__':
    unittest.main()
