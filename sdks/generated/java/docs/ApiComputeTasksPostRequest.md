# ApiComputeTasksPostRequest

## Properties

| Name                     | Type                                                                                                    | Description          | Notes      |
| ------------------------ | ------------------------------------------------------------------------------------------------------- | -------------------- | ---------- |
| **taskType**             | [**TaskTypeEnum**](#TaskTypeEnum)                                                                       | Type of compute task |            |
| **priority**             | [**PriorityEnum**](#PriorityEnum)                                                                       | Task priority level  | [optional] |
| **resourceRequirements** | [**ApiComputeTasksPostRequestResourceRequirements**](ApiComputeTasksPostRequestResourceRequirements.md) |                      | [optional] |

## Enum: TaskTypeEnum

| Name                   | Value                              |
| ---------------------- | ---------------------------------- |
| MACHINE_LEARNING       | &quot;machine-learning&quot;       |
| DATA_PROCESSING        | &quot;data-processing&quot;        |
| SCIENTIFIC_COMPUTATION | &quot;scientific-computation&quot; |
| RENDERING              | &quot;rendering&quot;              |

## Enum: PriorityEnum

| Name   | Value              |
| ------ | ------------------ |
| LOW    | &quot;low&quot;    |
| MEDIUM | &quot;medium&quot; |
| HIGH   | &quot;high&quot;   |
| URGENT | &quot;urgent&quot; |
