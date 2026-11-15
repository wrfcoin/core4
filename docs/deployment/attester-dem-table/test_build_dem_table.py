import json
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import build_dem_table as b  # noqa: E402

DEM = 247.9


class BandTest(unittest.TestCase):
    def test_edges_mirror_the_attester(self):
        self.assertTrue(b.within_band(DEM - 50, DEM))
        self.assertTrue(b.within_band(DEM + 60, DEM))
        self.assertFalse(b.within_band(DEM - 50.1, DEM))
        self.assertFalse(b.within_band(DEM + 60.1, DEM))
        self.assertTrue(b.within_band(227.7, DEM))  # the reference station: 20.2 m below its DEM
        for laundered in (453.0, 653.0, -1.0):
            self.assertFalse(b.within_band(laundered, DEM))
        self.assertFalse(b.within_band(float('nan'), DEM))
        self.assertFalse(b.within_band(None, DEM))

    def test_coarse_rounds_half_away_from_zero_like_rust(self):
        self.assertEqual(b.coarse(0.125), 0.13)  # Python round() would give 0.12 (half to even)
        self.assertEqual(b.coarse(-0.125), -0.13)
        self.assertEqual(b.coarse(-92.1031), -92.1)
        self.assertEqual(b.coarse(46.8123), 46.81)

    def test_epqs_no_data_and_garbage_are_none(self):
        self.assertEqual(b.parse_dem_value('{"value": "247.9"}'), 247.9)
        self.assertIsNone(b.parse_dem_value('{"value": -1000000}'))
        self.assertIsNone(b.parse_dem_value('{"value": ""}'))
        self.assertIsNone(b.parse_dem_value('not json'))


class TableTest(unittest.TestCase):
    STATIONS = [
        {'station_id': 'B', 'latitude': 46.8123, 'longitude': -92.1031, 'elevation_meters': 453.0},
        {'station_id': 'A', 'latitude': 46.8123, 'longitude': -92.1031, 'elevation_meters': 227.7},
        {'station_id': 'C', 'latitude': 46.0, 'longitude': -92.0, 'elevation_meters': None},
        {'station_id': 'D', 'latitude': 46.0, 'longitude': -92.0, 'elevation_meters': 200.0},
    ]

    def test_build_rounds_coordinates_and_skips_unavailable(self):
        lookup = lambda lat, lon: None if lat == 46.0 else DEM  # noqa: E731
        table, rows = b.build_table(self.STATIONS, lookup)
        self.assertEqual(table['version'], 1)
        self.assertEqual([s['station_id'] for s in table['stations']], ['A', 'B'])
        self.assertEqual(table['stations'][0]['latitude'], 46.81)
        self.assertEqual(table['stations'][0]['longitude'], -92.1)
        verdicts = {r[0]: r[3] for r in rows}
        self.assertEqual(verdicts['A'], 'IN')
        self.assertTrue(verdicts['B'].startswith('OUT'))
        self.assertTrue(verdicts['C'].startswith('NO_ELEVATION'))
        self.assertTrue(verdicts['D'].startswith('DEM_UNAVAILABLE'))

    def test_main_from_registry_file_prints_no_coordinates(self):
        with tempfile.TemporaryDirectory() as d:
            reg = os.path.join(d, 'registry.json')
            out = os.path.join(d, 'dem.json')
            with open(reg, 'w') as f:
                json.dump({'stations': self.STATIONS}, f)
            with mock.patch.object(b, 'epqs_lookup', lambda lat, lon: DEM), \
                    mock.patch('sys.stdout') as stdout:
                self.assertEqual(b.main(['--registry-file', reg, '--out', out, '--station', 'A']), 0)
            printed = ''.join(call.args[0] for call in stdout.write.call_args_list)
            self.assertNotIn('46.81', printed)
            self.assertNotIn('92.10', printed)
            with open(out) as f:
                table = json.load(f)
            self.assertEqual(table['stations'], [
                {'station_id': 'A', 'latitude': 46.81, 'longitude': -92.1, 'dem_ground_elevation_m': 247.9}])


if __name__ == '__main__':
    unittest.main()
