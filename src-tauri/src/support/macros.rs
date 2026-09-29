// Model定义宏（DeepSeek生成, DeepSeek优化）
#[macro_export]
macro_rules! api_model {
    (
        $(#[$struct_attr:meta])*
        $struct:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident : $type:ty
            ),+
            $(,)?
        }
    ) => {
        #[derive(Serialize, Deserialize, Debug, Clone, Type)]
        $(#[$struct_attr])*
        #[serde(rename_all = "camelCase")]
        pub struct $struct {
            $(
                $(#[$field_meta])*
                pub $field: $type
            ),+
        }
    };
}

// Api定义宏（DeepSeek生成）
#[macro_export]
macro_rules! api_commands {
    // 匹配多个函数定义的语法：用分号分隔每个定义
    (
        $(
            $name:ident(
                $($param:ident: $param_type:ty),*
            ) -> $return_type:ty
        );*
        $(;)?
    ) => {
        $(
            #[command]
            #[specta]
            pub fn $name(
                app_handle: AppHandle,
                id: &str,
                $($param: $param_type),*
            ) -> ApiResult<$return_type> {
                to_api_result(
                    app_handle
                        .get_client(id)
                        .and_then(|client| client.$name($($param),*))
                )
            }
        )*
    };
}
