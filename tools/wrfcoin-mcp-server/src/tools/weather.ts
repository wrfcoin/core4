import { notWired } from '../not-wired.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { ApiClient } from '../client.js';

export function registerWeatherTools(server: McpServer, _client: ApiClient): void {

  // 1. submit_weather_data - POST /api/weather/submit
  server.registerTool(
    'submit_weather_data',
    {
      title: 'Submit Weather Data',
      description: 'Not wired: backend submission exists, but this sensor-array payload and station-token integration are unavailable',
      inputSchema: z.object({
        stationId: z.string().describe('Weather station identifier'),
        sensorData: z.array(z.object({
          type: z.enum(['temperature', 'humidity', 'pressure', 'wind_speed', 'wind_direction', 'precipitation', 'visibility', 'uv_index']).describe('Sensor measurement type'),
          value: z.number().describe('Sensor reading value'),
          unit: z.string().describe('Unit of measurement (e.g., "celsius", "percent", "hPa")'),
        })).min(1).describe('Array of sensor readings to submit'),
        latitude: z.number().min(-90).max(90).optional().describe('Station latitude'),
        longitude: z.number().min(-180).max(180).optional().describe('Station longitude'),
      }),
    },
    async () => notWired('submit_weather_data', 'backend submission exists, but this sensor-array payload and station-token integration are unavailable')
  );

  // 2. query_weather_data - GET /api/weather/data
  server.registerTool(
    'query_weather_data',
    {
      title: 'Query Weather Data',
      description: 'Not wired: backend has no unfiltered weather-data query endpoint',
      inputSchema: z.object({
        stationId: z.string().optional().describe('Filter by station ID'),
        startDate: z.string().optional().describe('Start date (ISO 8601, e.g., "2026-01-01")'),
        endDate: z.string().optional().describe('End date (ISO 8601, e.g., "2026-01-31")'),
        dataType: z.enum(['temperature', 'humidity', 'pressure', 'wind', 'precipitation']).optional().describe('Filter by measurement type'),
      }),
    },
    async () => notWired('query_weather_data', 'backend has no unfiltered weather-data query endpoint')
  );

  // 3. get_weather_security_status - GET /api/weather/security-status
  server.registerTool(
    'get_weather_security_status',
    {
      title: 'Weather Security Status',
      description: 'Not wired: backend has no weather security-status endpoint',
      inputSchema: z.object({}),
    },
    async () => notWired('get_weather_security_status', 'backend has no weather security-status endpoint')
  );
}
