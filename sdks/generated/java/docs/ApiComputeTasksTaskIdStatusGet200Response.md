# ApiComputeTasksTaskIdStatusGet200Response

## Properties

| Name                    | Type                                                                                                                    | Description | Notes      |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------- | ---------- |
| **taskId**              | **String**                                                                                                              |             | [optional] |
| **status**              | [**StatusEnum**](#StatusEnum)                                                                                           |             | [optional] |
| **progress**            | **Integer**                                                                                                             |             | [optional] |
| **provider**            | **String**                                                                                                              |             | [optional] |
| **estimatedCompletion** | **String**                                                                                                              |             | [optional] |
| **resourceUsage**       | [**ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage**](ApiComputeTasksTaskIdStatusGet200ResponseResourceUsage.md) |             | [optional] |
| **cost**                | **String**                                                                                                              |             | [optional] |

## Enum: StatusEnum

| Name      | Value                 |
| --------- | --------------------- |
| QUEUED    | &quot;queued&quot;    |
| RUNNING   | &quot;running&quot;   |
| COMPLETED | &quot;completed&quot; |
| FAILED    | &quot;failed&quot;    |
