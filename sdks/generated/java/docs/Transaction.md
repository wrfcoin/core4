# Transaction

## Properties

| Name                 | Type                          | Description                       | Notes      |
| -------------------- | ----------------------------- | --------------------------------- | ---------- |
| **hash**             | **String**                    | Transaction hash                  | [optional] |
| **from**             | **String**                    | Sender address                    | [optional] |
| **to**               | **String**                    | Recipient address                 | [optional] |
| **value**            | **String**                    | Transaction value in wei          | [optional] |
| **gas**              | **Integer**                   | Gas limit                         | [optional] |
| **gasPrice**         | **String**                    | Gas price in wei                  | [optional] |
| **gasUsed**          | **Integer**                   | Gas used                          | [optional] |
| **nonce**            | **Integer**                   | Transaction nonce                 | [optional] |
| **blockHash**        | **String**                    | Block hash containing transaction | [optional] |
| **blockNumber**      | **Integer**                   | Block number                      | [optional] |
| **transactionIndex** | **Integer**                   | Transaction index in block        | [optional] |
| **status**           | [**StatusEnum**](#StatusEnum) |                                   | [optional] |
| **timestamp**        | **Integer**                   | Transaction timestamp             | [optional] |

## Enum: StatusEnum

| Name      | Value                 |
| --------- | --------------------- |
| PENDING   | &quot;pending&quot;   |
| CONFIRMED | &quot;confirmed&quot; |
| FAILED    | &quot;failed&quot;    |
