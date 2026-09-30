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

/// 单机 `mock_data`。`Commands` 不能写成 trait 方法，所以用宏在 `impl_single` 里展开。
/// 集群的造数是手写的 `ClusterPipeline`，不要并进这个宏。
#[macro_export]
macro_rules! implement_pipeline_commands {
    ($struct_name:ident) => {
        fn mock_data(&self, count: u64) -> $crate::support::util::AnyResult<()> {
            let mut pipe = $struct_name::with_capacity(count as usize);
            for _ in 0..count {
                let key = format!(
                    "redis-me-mock:string:{}",
                    $crate::support::util::random_string(10)
                );
                pipe.set(&key, $crate::support::util::random_string(10))
                    .ignore();

                let field_count = $crate::support::util::random_range(3, 200);
                let key = format!(
                    "redis-me-mock:hash:{}",
                    $crate::support::util::random_string(10)
                );
                for x in 0..field_count {
                    pipe.hset(
                        &key,
                        format!("key{x}"),
                        $crate::support::util::random_string(10),
                    )
                    .ignore();
                }

                let key = format!(
                    "redis-me-mock:list:{}",
                    $crate::support::util::random_string(10)
                );
                for _ in 0..field_count {
                    pipe.rpush(&key, $crate::support::util::random_string(10))
                        .ignore();
                }

                let key = format!(
                    "redis-me-mock:set:{}",
                    $crate::support::util::random_string(10)
                );
                for _ in 0..field_count {
                    pipe.sadd(&key, $crate::support::util::random_string(10))
                        .ignore();
                }

                let key = format!(
                    "redis-me-mock:zset:{}",
                    $crate::support::util::random_string(10)
                );
                for _ in 0..field_count {
                    pipe.zadd(
                        &key,
                        $crate::support::util::random_string(10),
                        $crate::support::util::random_range(1, 100),
                    )
                    .ignore();
                }
            }

            let mut conn = self.get_conn()?;
            let _: () = pipe.query(&mut conn)?;
            Ok(())
        }
    };
}
