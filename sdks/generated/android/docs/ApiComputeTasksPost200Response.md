# ApiComputeTasksPost200Response

## Properties

| Name                    | Type                          | Description | Notes      |
| ----------------------- | ----------------------------- | ----------- | ---------- |
| **success**             | **Boolean**                   |             | [optional] |
| **taskId**              | **String**                    |             | [optional] |
| **status**              | [**StatusEnum**](#StatusEnum) |             | [optional] |
| **assignedProvider**    | **String**                    |             | [optional] |
| **estimatedCompletion** | **String**                    |             | [optional] |
| **cost**                | **String**                    |             | [optional] |
| **queuePosition**       | **Integer**                   |             | [optional] |

## Enum: StatusEnum

| Name      | Value                 |
| --------- | --------------------- |
| QUEUED    | &quot;queued&quot;    |
| RUNNING   | &quot;running&quot;   |
| COMPLETED | &quot;completed&quot; |
| FAILED    | &quot;failed&quot;    |
