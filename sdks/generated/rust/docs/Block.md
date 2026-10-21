# Block

## Properties

| Name                  | Type               | Description                     | Notes      |
| --------------------- | ------------------ | ------------------------------- | ---------- |
| **hash**              | Option<**String**> | Block hash                      | [optional] |
| **index**             | Option<**i32**>    | Block number                    | [optional] |
| **previous_hash**     | Option<**String**> | Previous block hash             | [optional] |
| **timestamp**         | Option<**i32**>    | Block timestamp                 | [optional] |
| **merkle_root**       | Option<**String**> | Merkle root of transactions     | [optional] |
| **nonce**             | Option<**i32**>    | Mining nonce                    | [optional] |
| **difficulty**        | Option<**f64**>    | Block difficulty                | [optional] |
| **transaction_count** | Option<**i32**>    | Number of transactions in block | [optional] |
| **size**              | Option<**i32**>    | Block size in bytes             | [optional] |
| **gas_used**          | Option<**i32**>    | Total gas used in block         | [optional] |
| **gas_limit**         | Option<**i32**>    | Block gas limit                 | [optional] |

[[Back to Model list]](../README.md#documentation-for-models)
[[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)
