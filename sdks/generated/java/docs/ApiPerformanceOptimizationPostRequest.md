# ApiPerformanceOptimizationPostRequest

## Properties

| Name         | Type                          | Description                      | Notes      |
| ------------ | ----------------------------- | -------------------------------- | ---------- |
| **target**   | [**TargetEnum**](#TargetEnum) |                                  | [optional] |
| **level**    | [**LevelEnum**](#LevelEnum)   |                                  | [optional] |
| **duration** | **Integer**                   | Optimization duration in minutes | [optional] |

## Enum: TargetEnum

| Name     | Value                |
| -------- | -------------------- |
| DATABASE | &quot;database&quot; |
| NETWORK  | &quot;network&quot;  |
| COMPUTE  | &quot;compute&quot;  |
| STORAGE  | &quot;storage&quot;  |

## Enum: LevelEnum

| Name       | Value                  |
| ---------- | ---------------------- |
| LOW        | &quot;low&quot;        |
| MEDIUM     | &quot;medium&quot;     |
| HIGH       | &quot;high&quot;       |
| AGGRESSIVE | &quot;aggressive&quot; |
