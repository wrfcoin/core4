'use strict';

function cloneOption(definition) {
    return {
        description: '',
        required: false,
        default: undefined,
        includeInReport: false,
        ...definition,
    };
}

function normalizeOptionDefinitions(definitions) {
    return definitions.map((definition) => {
        const option = cloneOption(definition);
        if (!option.name || !option.type) {
            throw new Error(`Invalid option definition: ${JSON.stringify(definition)}`);
        }
        return option;
    });
}

function parseValue(option, rawValue, sourceLabel) {
    if (option.type === 'boolean') {
        if (rawValue === undefined) return true;
        if (rawValue === 'true' || rawValue === '1') return true;
        if (rawValue === 'false' || rawValue === '0') return false;
        throw new Error(`Option --${option.name} expects a boolean value when explicitly assigned (${sourceLabel}).`);
    }

    if (rawValue === undefined || rawValue === '') {
        throw new Error(`Option --${option.name} requires a value (${sourceLabel}).`);
    }

    if (option.type === 'int') {
        if (!/^-?\d+$/.test(rawValue)) {
            throw new Error(`Option --${option.name} expects an integer, received "${rawValue}" (${sourceLabel}).`);
        }
        return Number.parseInt(rawValue, 10);
    }

    return rawValue;
}

function parseCliArguments(argv, definitions, options) {
    const { allowUnknown = false } = options || {};
    const normalized = normalizeOptionDefinitions(definitions);
    const definitionMap = new Map(normalized.map((definition) => [definition.name, definition]));
    const values = {};
    const sources = {};
    const unknown = [];
    const errors = [];

    for (let index = 0; index < argv.length; index++) {
        const argument = argv[index];
        if (!argument.startsWith('--')) {
            errors.push(`Unexpected positional argument "${argument}".`);
            continue;
        }

        const trimmed = argument.slice(2);
        const equalsIndex = trimmed.indexOf('=');
        const optionName = equalsIndex === -1 ? trimmed : trimmed.slice(0, equalsIndex);
        const inlineValue = equalsIndex === -1 ? undefined : trimmed.slice(equalsIndex + 1);
        const definition = definitionMap.get(optionName);

        if (!definition) {
            if (allowUnknown) {
                unknown.push(argument);
                continue;
            }
            errors.push(`Unknown option "${argument}".`);
            continue;
        }

        let rawValue = inlineValue;
        if (definition.type !== 'boolean' && rawValue === undefined) {
            const nextValue = argv[index + 1];
            if (nextValue === undefined || nextValue.startsWith('--')) {
                errors.push(`Option --${optionName} requires a value.`);
                continue;
            }
            rawValue = nextValue;
            index += 1;
        }

        try {
            values[optionName] = parseValue(definition, rawValue, 'CLI');
            sources[optionName] = 'cli';
        } catch (error) {
            errors.push(error.message);
        }
    }

    return { definitions: normalized, values, sources, unknown, errors, helpRequested: Boolean(values.help) };
}

function resolveOptionValues(parsed, env) {
    const resolved = { ...parsed.values };
    const sources = { ...parsed.sources };

    for (const definition of parsed.definitions) {
        if (resolved[definition.name] !== undefined) continue;

        if (definition.env && env[definition.env] !== undefined && env[definition.env] !== '') {
            resolved[definition.name] = parseValue(definition, env[definition.env], `env:${definition.env}`);
            sources[definition.name] = `env:${definition.env}`;
            continue;
        }

        if (definition.default !== undefined) {
            resolved[definition.name] = definition.default;
            sources[definition.name] = 'default';
        }
    }

    return { values: resolved, sources };
}

function buildUsage(scriptName, description, definitions) {
    const normalized = normalizeOptionDefinitions(definitions);
    const lines = [];

    if (description) lines.push(description);
    lines.push(`Usage: node ${scriptName} [options]`);
    lines.push('');
    lines.push('Options:');

    for (const option of normalized) {
        let label = `  --${option.name}`;
        if (option.type === 'int') label += ' <int>';
        if (option.type === 'string') label += ' <value>';

        const details = [];
        if (option.description) details.push(option.description);
        if (option.env) details.push(`env: ${option.env}`);
        if (option.default !== undefined) details.push(`default: ${option.default}`);
        if (option.required) details.push('required');

        lines.push(details.length > 0 ? `${label.padEnd(28)}${details.join('; ')}` : label);
    }

    return `${lines.join('\n')}\n`;
}

function maskToken(token) {
    if (!token) return 'missing';
    if (token.length <= 8) return 'set';
    return `${token.slice(0, 4)}…${token.slice(-4)}`;
}

function formatRuntimeReport(config, extras) {
    const lines = [
        `ℹ️  Runtime: api=${config.apiBaseUrl} (host=${config.apiHost} via ${config.sources.host || 'n/a'}, port=${config.apiPort} via ${config.sources.port || 'n/a'})`,
        `ℹ️  Operator token: ${maskToken(config.operatorToken)} (${config.sources.token || 'missing'})`,
    ];

    const reportedExtras = extras || {};
    for (const [name, value] of Object.entries(reportedExtras)) {
        lines.push(`ℹ️  ${name}: ${value}`);
    }

    return lines.join('\n');
}

function exportToEnvironment(config) {
    process.env.WRFCOIN_API_HOST = String(config.apiHost);
    process.env.WRFCOIN_API_PORT = String(config.apiPort);
    if (config.operatorToken) {
        process.env.WRFCOIN_OPERATOR_TOKEN = config.operatorToken;
    }
}

function loadTestConfig(specification) {
    const {
        argv = process.argv.slice(2),
        env = process.env,
        scriptName,
        description,
        requireToken = false,
        extraOptions = [],
        allowUnknown = false,
        stdout = process.stdout,
        stderr = process.stderr,
    } = specification || {};

    const definitions = [
        { name: 'help', type: 'boolean', description: 'Show this help message' },
        { name: 'host', type: 'string', env: 'WRFCOIN_API_HOST', default: '127.0.0.1', description: 'WRFCoin API host' },
        { name: 'port', type: 'int', env: 'WRFCOIN_API_PORT', default: 8556, description: 'WRFCoin API port' },
        { name: 'token', type: 'string', env: 'WRFCOIN_OPERATOR_TOKEN', description: 'Operator token for authenticated endpoints' },
        ...extraOptions,
    ];

    const parsed = parseCliArguments(argv, definitions, { allowUnknown });
    if (parsed.helpRequested) {
        stdout.write(buildUsage(scriptName, description, definitions));
        process.exit(0);
    }

    if (parsed.errors.length > 0) {
        stderr.write(`${parsed.errors.join('\n')}\n\n${buildUsage(scriptName, description, definitions)}`);
        process.exit(1);
    }

    let resolved;
    try {
        resolved = resolveOptionValues(parsed, env);
    } catch (error) {
        stderr.write(`${error.message}\n\n${buildUsage(scriptName, description, definitions)}`);
        process.exit(1);
    }

    if (requireToken && !resolved.values.token) {
        stderr.write(`Missing required operator token. Provide --token or set WRFCOIN_OPERATOR_TOKEN.\n\n${buildUsage(scriptName, description, definitions)}`);
        process.exit(1);
    }

    return {
        scriptName,
        description,
        apiHost: resolved.values.host,
        apiPort: resolved.values.port,
        operatorToken: resolved.values.token,
        apiBaseUrl: `http://${resolved.values.host}:${resolved.values.port}`,
        values: resolved.values,
        sources: resolved.sources,
        definitions,
        unknown: parsed.unknown,
    };
}

module.exports = {
    buildUsage,
    exportToEnvironment,
    formatRuntimeReport,
    loadTestConfig,
    parseCliArguments,
    resolveOptionValues,
};
