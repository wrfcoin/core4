'use strict';

/**
 * Credential-free validation for the private Ambient station roster.
 *
 * This module deliberately accepts only an opaque provider binding reference. Provider keys are
 * resolved by the separately authorized runtime boundary and never belong in
 * this configuration object or in validation errors.
 */

const MAC_PATTERN = /^[0-9a-f]{2}(?::[0-9a-f]{2}){5}$/i;
const OPAQUE_REF_PATTERN = /^(?:[0-9a-f]{32,}|[A-Za-z0-9+/]{32,}={0,2})$/;
const ALLOWED_ROSTER_FIELDS = new Set([
    'provider', 'stationId', 'ownerAddress', 'mac', 'model', 'device_model',
    'consent', 'consentStatus', 'coarseMetadata', 'coarseMetadataStatus', 'bindingRef',
]);

function normalizeMac(value) {
    if (typeof value !== 'string' || !MAC_PATTERN.test(value)) return null;
    return value.toUpperCase();
}

function normalizeModel(value) {
    if (typeof value !== 'string') return null;
    const model = value.trim();
    return model.length > 0 && model.length <= 128 ? model : null;
}

function isApprovedConsent(entry) {
    return entry.consent === true || entry.consentStatus === 'approved';
}

function isApprovedCoarseMetadata(entry) {
    return entry.coarseMetadataStatus === 'approved'
        || entry.coarseMetadata?.status === 'approved';
}

function validateStationRosterEntry(entry, index = 0) {
    const errors = [];
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
        return { ok: false, errors: [{ index, code: 'malformed_entry' }] };
    }

    if (String(entry.provider || '').toLowerCase() !== 'ambient') errors.push('unsupported_provider');
    if (typeof entry.stationId !== 'string' || entry.stationId.trim() === '') errors.push('missing_station_id');
    if (typeof entry.ownerAddress !== 'string' || entry.ownerAddress.trim() === '') errors.push('missing_owner');

    const mac = normalizeMac(entry.mac);
    if (!mac) errors.push(typeof entry.mac === 'undefined' ? 'missing_mac' : 'malformed_mac');

    const model = normalizeModel(entry.model ?? entry.device_model);
    if (!model) errors.push(typeof (entry.model ?? entry.device_model) === 'undefined' ? 'missing_model' : 'malformed_model');

    if (!isApprovedConsent(entry)) errors.push('consent_not_approved');
    if (!isApprovedCoarseMetadata(entry)) errors.push('coarse_metadata_not_approved');

    if (typeof entry.bindingRef !== 'string' || entry.bindingRef.trim() === '') {
        errors.push('missing_binding_ref');
    } else if (OPAQUE_REF_PATTERN.test(entry.bindingRef.trim())) {
        errors.push('binding_value_forbidden');
    }
    if (Object.keys(entry).some((field) => !ALLOWED_ROSTER_FIELDS.has(field))) {
        errors.push('unsupported_roster_field');
    }

    if (errors.length > 0) return { ok: false, errors: errors.map((code) => ({ index, code })) };

    return {
        ok: true,
        entry: {
            provider: 'ambient',
            stationId: entry.stationId.trim(),
            ownerAddress: entry.ownerAddress.trim(),
            mac,
            model,
            consentStatus: 'approved',
            coarseMetadataStatus: 'approved',
            bindingRef: entry.bindingRef.trim(),
        },
    };
}

function validateStationRoster(entries) {
    if (!Array.isArray(entries)) {
        return { ok: false, entries: [], errors: [{ index: -1, code: 'malformed_roster' }] };
    }

    const normalized = [];
    const errors = [];
    entries.forEach((entry, index) => {
        const result = validateStationRosterEntry(entry, index);
        if (!result.ok) errors.push(...result.errors);
        else normalized.push(result.entry);
    });

    const stationIds = new Map();
    const macs = new Map();
    normalized.forEach((entry, normalizedIndex) => {
        if (stationIds.has(entry.stationId)) {
            errors.push({ index: normalizedIndex, code: 'duplicate_station_id' });
        } else {
            stationIds.set(entry.stationId, normalizedIndex);
        }
        if (macs.has(entry.mac)) {
            errors.push({ index: normalizedIndex, code: 'duplicate_mac' });
        } else {
            macs.set(entry.mac, normalizedIndex);
        }
    });

    return { ok: errors.length === 0, entries: errors.length === 0 ? normalized : [], errors };
}

function deviceModel(device) {
    return normalizeModel(device?.model ?? device?.deviceModel ?? device?.info?.model ?? device?.info?.device_model);
}

function selectExactDevice(entry, devices) {
    const mac = normalizeMac(entry?.mac);
    const model = normalizeModel(entry?.model ?? entry?.device_model);
    if (!mac || !model) return { ok: false, code: 'invalid_selector' };
    if (!Array.isArray(devices)) return { ok: false, code: 'invalid_device_response' };

    const matches = devices.filter((device) => normalizeMac(device?.macAddress) === mac);
    if (matches.length === 0) return { ok: false, code: 'device_mismatch' };
    if (matches.length !== 1) return { ok: false, code: 'ambiguous_device' };

    const returnedModel = deviceModel(matches[0]);
    // `stationtype` is Ambient's usual model/firmware carrier — surfaced for
    // diagnosis only, deliberately NOT consulted by the comparison: it is a
    // firmware string ("AMBWeatherPro_V5.1.9"), and comparing it against an
    // operator's "WS-2902" would only convert the old lockout into a
    // differently-worded one (#2736 option 1, rejected).
    const stationtype = typeof matches[0]?.lastData?.stationtype === 'string'
        ? matches[0].lastData.stationtype
        : null;

    // #2736 (option 2): the model binding is provenance-graded, not
    // unsatisfiable. Ambient's /v1/devices returns no model at all for real
    // hardware (measured 2026-08-05), so requiring provider corroboration
    // refused every contributor at `model_unverified` and #1732 multi-tenant
    // onboarding never worked for anyone.
    //
    //  - Provider supplies a model: compare exactly as before. A
    //    contradiction still REFUSES (`model_mismatch`, with what was
    //    expected vs returned) — the check keeps its teeth wherever there is
    //    something to check.
    //  - Provider supplies nothing: proceed on the roster's model, but as an
    //    `operator_asserted` binding rather than `provider_verified`. The
    //    caller carries that distinction into attribution metadata and logs,
    //    so the weaker binding is visible downstream (e.g. for a lower
    //    initial trust tier), never silently equivalent.
    //
    // The device universe here is already owner-scoped — /v1/devices is
    // authenticated by the contributor's own credentials — so MAC uniqueness
    // (roster `duplicate_mac` + `ambiguous_device` above) remains the
    // attribution boundary; the model comparison is a wrong-own-device typo
    // guard, applied whenever the provider gives us anything to check.
    if (!returnedModel) {
        return {
            ok: true,
            device: matches[0],
            model,
            modelBinding: 'operator_asserted',
            providerModel: null,
            stationtype,
        };
    }
    if (returnedModel.toLowerCase() !== model.toLowerCase()) {
        return {
            ok: false,
            code: 'model_mismatch',
            detail: { expectedModel: model, returnedModel, stationtype },
        };
    }

    return {
        ok: true,
        device: matches[0],
        model: returnedModel,
        modelBinding: 'provider_verified',
        providerModel: returnedModel,
        stationtype,
    };
}

function observationTimestampMs(observation) {
    if (!observation || typeof observation !== 'object') return null;
    const candidate = observation.recorded_at ?? observation.recordedAt
        ?? observation.dateutc ?? observation.timestamp;
    if (typeof candidate === 'number' && Number.isFinite(candidate)) {
        return candidate < 1e12 ? candidate * 1000 : candidate;
    }
    if (typeof candidate === 'string' && candidate.trim() !== '') {
        const parsed = Date.parse(candidate);
        return Number.isFinite(parsed) ? parsed : null;
    }
    return null;
}

function verifyObservationFreshness(observation, options = {}) {
    const nowMs = Number.isFinite(options.nowMs) ? options.nowMs : Date.now();
    const maxAgeMinutes = Number.isFinite(options.maxAgeMinutes) ? options.maxAgeMinutes : 30;
    if (maxAgeMinutes < 0) return { ok: false, code: 'invalid_freshness_config' };

    const timestampMs = observationTimestampMs(observation);
    if (timestampMs === null) return { ok: false, code: 'invalid_observation_timestamp' };
    const ageMinutes = (nowMs - timestampMs) / 60_000;
    if (ageMinutes < 0) return { ok: false, code: 'future_observation' };
    if (ageMinutes > maxAgeMinutes) return { ok: false, code: 'stale_observation', ageMinutes };
    return { ok: true, ageMinutes };
}

module.exports = {
    normalizeMac,
    validateStationRosterEntry,
    validateStationRoster,
    selectExactDevice,
    verifyObservationFreshness,
};
