# 值面板扩展操作清单（RedisValue.vue）

> 状态：待分析 / 待排期  
> 关联文件：`src/views/tab/RedisValue.vue`  
> 背景：当前值面板已支持 LPOP/RPOP/SPOP/ZPOPMIN/ZPOPMAX 等弹出操作，以及排序切换、范围过滤、HTTL、Groups、复制为命令等功能。本清单按 Redis 数据类型梳理仍可扩展的操作，供后续迭代决策。

---

## 一、List

| 操作                 | 命令                                   | 说明                       | 优先级 |
| -------------------- | -------------------------------------- | -------------------------- | ------ |
| **LTRIM**            | `LTRIM key start stop`                 | 裁剪列表，保留指定索引范围 | 高     |
| **LINSERT**          | `LINSERT key BEFORE/AFTER pivot value` | 在指定元素前/后插入新元素  | 中     |
| **LSET**             | `LSET key index value`                 | 修改指定索引位置的值       | 中     |
| **批量 LPUSH/RPUSH** | `LPUSH/RPUSH key value [value ...]`    | 头部/尾部批量插入          | 中     |

---

## 二、Hash

| 操作             | 命令                               | 说明                       | 优先级 |
| ---------------- | ---------------------------------- | -------------------------- | ------ |
| **HINCRBY**      | `HINCRBY key field increment`      | 字段值原子自增（整数）     | 高     |
| **HINCRBYFLOAT** | `HINCRBYFLOAT key field increment` | 字段值原子自增（浮点数）   | 中     |
| **HSTRLEN**      | `HSTRLEN key field`                | 获取字段值的长度           | 低     |
| **HEXISTS**      | `HEXISTS key field`                | 检查字段是否存在（查询类） | 低     |

---

## 三、Set

| 操作             | 命令                           | 说明                        | 优先级 |
| ---------------- | ------------------------------ | --------------------------- | ------ |
| **SRANDMEMBER**  | `SRANDMEMBER key [count]`      | 随机获取 N 个成员（不删除） | 高     |
| **SISMEMBER**    | `SISMEMBER key member`         | 检查成员是否存在（查询类）  | 低     |
| **SREM**（批量） | `SREM key member [member ...]` | 批量移除成员                | 中     |

---

## 四、ZSet

| 操作                 | 命令                              | 说明                     | 优先级 |
| -------------------- | --------------------------------- | ------------------------ | ------ |
| **ZINCRBY**          | `ZINCRBY key increment member`    | 增加成员的分数           | 高     |
| **ZRANK / ZREVRANK** | `ZRANK/ZREVRANK key member`       | 查看成员排名（表格新列） | 中     |
| **ZREMRANGEBYRANK**  | `ZREMRANGEBYRANK key start stop`  | 按排名范围删除           | 中     |
| **ZREMRANGEBYSCORE** | `ZREMRANGEBYSCORE key min max`    | 按分数范围删除           | 中     |
| **ZMSCORE**          | `ZMSCORE key member [member ...]` | 批量获取分数             | 低     |

---

## 五、String

| 操作                | 命令                          | 说明                 | 优先级 |
| ------------------- | ----------------------------- | -------------------- | ------ |
| **APPEND**          | `APPEND key value`            | 追加内容到字符串尾部 | 高     |
| **INCR / DECR**     | `INCR/DECR key`               | 原子自增/自减        | 高     |
| **INCRBY / DECRBY** | `INCRBY/DECRBY key decrement` | 按指定步长增减       | 中     |
| **GETRANGE**        | `GETRANGE key start end`      | 获取子串（局部预览） | 低     |
| **SETRANGE**        | `SETRANGE key offset value`   | 从指定偏移覆盖子串   | 低     |

---

## 六、Stream

| 操作                   | 命令                                                    | 说明                     | 优先级 |
| ---------------------- | ------------------------------------------------------- | ------------------------ | ------ |
| **XACK**               | `XACK key group id [id ...]`                            | 确认消息已消费           | 高     |
| **XPENDING**           | `XPENDING key group [[start] [end] [count] [consumer]]` | 查看待处理消息概况       | 高     |
| **XREAD / XREADGROUP** | `XREAD/XREADGROUP ...`                                  | 读取新消息（消费者模拟） | 中     |
| **XDEL**               | `XDEL key id [id ...]`                                  | 删除指定 ID 的消息       | 中     |
| **XTRIM**              | `XTRIM key MAXLEN [~] count`                            | 裁剪 Stream 长度         | 中     |

---

## 七、通用/键级扩展

| 操作                | 命令                               | 说明                   | 优先级 |
| ------------------- | ---------------------------------- | ---------------------- | ------ |
| **OBJECT ENCODING** | `OBJECT ENCODING key`              | 查看键的内部编码       | 中     |
| **OBJECT IDLETIME** | `OBJECT IDLETIME key`              | 键的空闲时间（秒）     | 低     |
| **OBJECT FREQ**     | `OBJECT FREQ key`                  | LFU 访问频率           | 低     |
| **DUMP**            | `DUMP key`                         | 序列化键（二进制导出） | 中     |
| **RESTORE**         | `RESTORE key ttl serialized-value` | 反序列化恢复键         | 中     |
| **TOUCH**           | `TOUCH key [key ...]`              | 更新键的访问时间       | 低     |

---

## 八、优先级汇总（建议迭代顺序）

### P0（下一迭代，高价值低复杂度）

- **List LTRIM**
- **String APPEND / INCR / DECR**
- **ZSet ZINCRBY**
- **Stream XACK / XPENDING**

### P1（随后迭代）

- **Hash HINCRBY / HINCRBYFLOAT**
- **Set SRANDMEMBER**
- **ZSet ZRANK / ZREVRANK（表格列扩展）**
- **Stream XDEL / XTRIM**
- **通用 OBJECT ENCODING / DUMP / RESTORE**

### P2（中长期）

- **List LINSERT / LSET（批量编辑场景）**
- **String GETRANGE / SETRANGE**
- **ZSet ZREMRANGEBYRANK / ZREMRANGEBYSCORE**
- **Stream XREAD / XREADGROUP**
- **通用 OBJECT IDLETIME / FREQ / TOUCH**

---

## 九、实现注意事项

1. **命令兼容性**：部分命令（如 `TOUCH`、`ZMSCORE`）在较旧版本 Redis 或某些分支（如 Valkey）中可能不存在，需做版本/能力检测。
2. **危险操作确认**：LTRIM、ZREMRANGEBYRANK、XTRIM 等批量删除类操作需统一双确认机制。
3. **后端 API 复用**：多数操作可复用现有 `field_pop` / `field_set` / `field_del` 的接口范式，新增命令模式即可。
4. **前端交互**：建议在现有「fieldCommands」下拉菜单中按类型分组扩展，避免按钮过多。
