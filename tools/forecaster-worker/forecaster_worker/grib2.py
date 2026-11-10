"""A very small GRIB2 reader: just enough for the point boxes the NOMADS grib filter cuts.

Supported, and checked on every message: regular latitude/longitude grid (grid template 3.0),
analysis-or-forecast product (product template 4.0), simple packing (data template 5.0), no bitmap.
Anything else raises GribError and the caller skips the file. No numpy, no eccodes.
"""
import struct
from dataclasses import dataclass


class GribError(Exception):
    pass


@dataclass
class Field:
    discipline: int
    category: int
    number: int
    level_type: int
    level_value: float
    forecast_hours: int
    reference_time: tuple  # (year, month, day, hour)
    ni: int
    nj: int
    lat0: float      # latitude of row 0
    lon0: float      # longitude of column 0, degrees east in [0, 360)
    dlat: float      # signed step between rows
    dlon: float      # step between columns (east)
    packed: tuple    # (data bytes, bit width, reference, binary scale, decimal scale); unpacked on first use
    _values: list | None = None

    @property
    def values(self) -> list:
        """Row-major, row 0 first. Unpacked only when a field is actually used."""
        if self._values is None:
            data, nbits, reference, scale_b, scale_d = self.packed
            self._values = [(reference + x * scale_b) / scale_d for x in _unpack_bits(data, nbits, self.ni * self.nj)]
        return self._values

    def at(self, lat: float, lon: float) -> float:
        """Bilinear value at a point inside the grid."""
        try:
            return self._at(lat, lon)
        except GribError:
            raise
        except (IndexError, ZeroDivisionError, OverflowError, ValueError, ArithmeticError, MemoryError) as error:
            raise GribError(f"damaged GRIB grid ({type(error).__name__})") from None

    def _at(self, lat: float, lon: float) -> float:
        y = (lat - self.lat0) / self.dlat
        x = ((lon - self.lon0) % 360.0) / self.dlon
        if not (-1e-6 <= y <= self.nj - 1 + 1e-6 and -1e-6 <= x <= self.ni - 1 + 1e-6):
            raise GribError("the point is outside the downloaded box")
        y = min(max(y, 0.0), self.nj - 1.0)
        x = min(max(x, 0.0), self.ni - 1.0)
        j0, i0 = min(int(y), self.nj - 2) if self.nj > 1 else 0, min(int(x), self.ni - 2) if self.ni > 1 else 0
        j1, i1 = min(j0 + 1, self.nj - 1), min(i0 + 1, self.ni - 1)
        fy, fx = y - j0, x - i0
        v = self.values
        top = v[j0 * self.ni + i0] * (1 - fx) + v[j0 * self.ni + i1] * fx
        bottom = v[j1 * self.ni + i0] * (1 - fx) + v[j1 * self.ni + i1] * fx
        return top * (1 - fy) + bottom * fy


def _signed(raw: bytes) -> int:
    """GRIB sign-and-magnitude integer."""
    value = int.from_bytes(raw, "big")
    top = 1 << (8 * len(raw) - 1)
    return -(value & (top - 1)) if value & top else value


def _unpack_bits(data: bytes, nbits: int, count: int) -> list:
    if nbits == 0:
        return [0] * count
    if len(data) * 8 < nbits * count:
        raise GribError("data section is shorter than the grid")
    big = int.from_bytes(data, "big")
    total = len(data) * 8
    mask = (1 << nbits) - 1
    return [(big >> (total - nbits * (k + 1))) & mask for k in range(count)]


MAX_GRID_POINTS = 10_000     # a point box is 25 points; anything huge is not what the worker asked for
MAX_MESSAGES = 8             # the worker asks for two variables on two levels: three or four messages
MAX_TOTAL_POINTS = 20_000    # across all messages of one file
MAX_FILE_BYTES = 200_000


def read_fields(blob: bytes) -> list:
    """Decode every message, or raise GribError. No other exception leaves this function."""
    try:
        return _read_fields(blob)
    except GribError:
        raise
    except (IndexError, ValueError, OverflowError, ZeroDivisionError, ArithmeticError, MemoryError, struct.error) as error:
        raise GribError(f"damaged GRIB file ({type(error).__name__})") from None


def _read_fields(blob: bytes) -> list:
    if len(blob) > MAX_FILE_BYTES:
        raise GribError("the file is far larger than a point box")
    fields = []
    points = 0
    pos = 0
    while pos < len(blob):
        if blob[pos:pos + 4] != b"GRIB":
            raise GribError("not a GRIB message")
        if blob[pos + 7] != 2:
            raise GribError("not GRIB edition 2")
        discipline = blob[pos + 6]
        total = struct.unpack(">Q", blob[pos + 8:pos + 16])[0]
        end = pos + total
        if total < 20 or end > len(blob) or blob[end - 4:end] != b"7777":
            raise GribError("truncated GRIB message")
        cursor = pos + 16
        sections = {}
        while cursor < end - 4:
            length, number = struct.unpack(">IB", blob[cursor:cursor + 5])
            if length < 5 or cursor + length > end - 4:
                raise GribError("bad section length")
            if number in sections and number != 2:
                raise GribError("messages with repeated sections are not supported")
            sections[number] = blob[cursor:cursor + length]
            cursor += length
        if len(fields) >= MAX_MESSAGES:
            raise GribError("the file holds far more messages than a point box")
        fields.append(_decode(discipline, sections))
        points += fields[-1].ni * fields[-1].nj
        if points > MAX_TOTAL_POINTS:
            raise GribError("the file holds far more points than a point box")
        pos = end
    if not fields:
        raise GribError("empty GRIB file")
    return fields


def _decode(discipline: int, sections: dict) -> Field:
    for needed in (1, 3, 4, 5, 6, 7):
        if needed not in sections:
            raise GribError(f"section {needed} is missing")
    s1, s3, s4, s5, s6, s7 = (sections[n] for n in (1, 3, 4, 5, 6, 7))
    reference = (struct.unpack(">H", s1[12:14])[0], s1[14], s1[15], s1[16])

    if struct.unpack(">H", s3[12:14])[0] != 0 or s3[10] != 0 or len(s3) < 72:
        raise GribError("only a regular latitude/longitude grid is supported")
    ni, nj = struct.unpack(">II", s3[30:38])
    basic, sub = struct.unpack(">II", s3[38:46])
    unit = 1e-6 if basic == 0 or sub in (0, 0xFFFFFFFF) else basic / sub
    la1, lo1 = _signed(s3[46:50]) * unit, _signed(s3[50:54]) * unit
    la2 = _signed(s3[55:59]) * unit
    di, dj = struct.unpack(">II", s3[63:71])
    scan = s3[71]
    if scan & 0x30:
        raise GribError("unsupported scanning mode")
    if ni < 1 or nj < 1 or ni * nj > MAX_GRID_POINTS:
        raise GribError("the grid is not a small point box")
    if ni * nj != struct.unpack(">I", s3[6:10])[0]:
        raise GribError("grid size does not match the point count")
    dlon = di * unit * (-1 if scan & 0x80 else 1)
    dlat = dj * unit * (1 if scan & 0x40 else -1)
    if nj > 1 and abs((la1 + dlat * (nj - 1)) - la2) > 1e-3:
        raise GribError("grid latitudes are inconsistent")
    if dlon <= 0 or (ni > 1 and di == 0) or (nj > 1 and dj == 0):
        raise GribError("unsupported scanning mode")
    if nj == 1:
        dlat = dlat or 1.0     # a single row: any non-zero step keeps the row index at 0
    if dlat == 0:
        raise GribError("grid latitudes are inconsistent")

    if struct.unpack(">H", s4[7:9])[0] != 0:
        raise GribError("only product template 4.0 is supported")
    category, number = s4[9], s4[10]
    time_unit, forecast_time = s4[17], struct.unpack(">I", s4[18:22])[0]
    if time_unit != 1:
        raise GribError("forecast time is not in hours")
    level_type = s4[22]
    level_value = _signed(s4[24:28]) / (10 ** _signed(s4[23:24])) if s4[23] != 0xFF else 0.0

    if struct.unpack(">H", s5[9:11])[0] != 0:
        raise GribError("only simple packing (data template 5.0) is supported")
    count = struct.unpack(">I", s5[5:9])[0]
    if count != ni * nj or s6[5] != 255:
        raise GribError("bitmaps are not supported")
    reference_value = struct.unpack(">f", s5[11:15])[0]
    binary, decimal, nbits = _signed(s5[15:17]), _signed(s5[17:19]), s5[19]
    if nbits > 32:
        raise GribError("unsupported bit width")
    data = bytes(s7[5:])
    if len(data) * 8 < nbits * count:
        raise GribError("data section is shorter than the grid")
    packed = (data, nbits, reference_value, 2.0 ** binary, 10.0 ** decimal)
    return Field(discipline, category, number, level_type, level_value, forecast_time, reference, ni, nj,
                 la1, lo1 % 360.0, dlat, dlon, packed)


def pick(fields: list, *, category: int, number: int, level_type: int, level_value: float | None = None) -> Field:
    for f in fields:
        if (f.discipline, f.category, f.number, f.level_type) == (0, category, number, level_type) \
                and (level_value is None or abs(f.level_value - level_value) < 1e-6):
            return f
    raise GribError("the wanted field is not in the file")
