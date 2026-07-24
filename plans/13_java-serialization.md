# JDK 序列化查看方案（JavaSerial）

> **实现状态**：✅ 已实现（只读，对齐 RedisInsight）  
> **关联 backlog**：[docs/zh/changelog/future.md](../docs/zh/changelog/future.md)  
> **关键代码**：`src/utils/javaserial/`（`index.ts` / `time.ts` / `collections.ts` / `misc.ts`）、`src/utils/format.ts`  
> **依赖**：`java-object-serialization` + `patches/java-object-serialization@0.1.2.patch`（Externalizable + TC_CLASS）

## 决策

| 项          | 结论                                                                                                                   |
| ----------- | ---------------------------------------------------------------------------------------------------------------------- |
| UI 名       | **JavaSerial**（view = `javaserial`）                                                                                  |
| 能力        | **仅查看**（编辑器只读，隐藏保存）                                                                                     |
| 顶层 String | 纯字符串展示                                                                                                           |
| 解析库      | `java-object-serialization`（Uint8Array，无需 Buffer polyfill）                                                        |
| 增强        | Date / sql.Date·Timestamp；`java.time`；record/POJO；常用集合与 BitSet / StringBuilder·Buffer / InetAddress / Calendar |
| 写回        | 不支持；复杂场景用自定义编解码 + 本机 Java                                                                             |

## 实现要点

- EXT 下拉：`StrJson` / `MsgPack` / `JavaSerial`
- wire：`base64`；仅 STRING 整键
- decode：`ObjectInputStream.readObject()` → 展示
- `canSave` 在 `javaserial` 时为 false（与 AnotherRDM readonly 一致）
- **LocalDate 等失败原因**：经 `java.time.Ser`（Externalizable）写出，上游库原对 Externalizable 直接 `unimplemented`
- **解法**：`pnpm patch` 支持已注册类的 `readExternal`；`javaserial/time.ts` 按 OpenJDK Ser 协议解析（嵌套字段一并生效）
- **EnumMap / EnumSet**：依赖 patch 对 `TC_CLASS`（`readNewClass`）的支持
- **魔数自动识别**：由 Auto 编码统一做（见 `detect-view-format.ts`）

## 类型覆盖复核

按「解析能否读通」与「展示是否友好」分开看。底层 `java-object-serialization`：String / 数组 / enum / 默认字段流（含 **Serializable record**）均可读；自定义 `writeObject` / Externalizable 的类型需注册 handler。

### 已增强（可读且展示友好）

| 类别 | 类型                                                                                                                                                                                                             |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 标量 | `String`；`Integer`/`Long`/`Double`/`Float`/`Boolean`/`Short`/`Byte`/`Character`（拆箱）                                                                                                                         |
| 数值 | `BigDecimal` / `BigInteger`（`$type` + 可读字符串）                                                                                                                                                              |
| ID   | `UUID`                                                                                                                                                                                                           |
| 日期 | `java.util.Date`；`java.sql.Date` / `Timestamp`；`Calendar` / `GregorianCalendar`                                                                                                                                |
| 时间 | `java.time.Ser` 全套常见类型：LocalDate/Time/DateTime、Instant、Zoned/Offset*、ZoneId/Offset、Year/YearMonth/MonthDay、Period、Duration                                                                          |
| 集合 | `ArrayList` / `LinkedList` / `Vector` / `Stack` / `ArrayDeque` / `PriorityQueue`；`HashMap` / `LinkedHashMap` / `TreeMap` / `ConcurrentHashMap` / `EnumMap`；`HashSet` / `LinkedHashSet` / `TreeSet` / `EnumSet` |
| 其它 | `BitSet`；`StringBuilder` / `StringBuffer`；`InetAddress`                                                                                                                                                        |
| 结构 | 任意默认序列化 **POJO**；**record**（Java 16+，流形态即字段列表，走 `$class` 扁平）；**enum**                                                                                                                    |

Seed：`test/serialization/JavaSerialSeed.java`（键前缀 `encoding:javaserial:`）。

### 通用路径可用（未特判，展示为 `$class` + 字段，一般够看）

- `Locale`、`Currency`、`SimpleTimeZone` 等普通 Serializable（`Optional` 本身不可序列化，不会出现在流里）
- `java.time` 里走 **enum** 的（如 `Month`、`DayOfWeek`）
- 对象/基本类型**数组**（库 `readNewArray` → JS 数组，再经 normalize）
- sealed class / 普通继承 POJO：无额外协议，与 POJO 相同

### JDK「新增」类型（25 之前）与序列化

- **Record（16）**：已支持（无需特判协议）
- **Sealed（17）**：不改变序列化形态 → 同 POJO
- **Sequenced Collections（21）**：接口层；实现类已覆盖常见 Linked\* / 有序集合
- Virtual threads / pattern matching 等：不进入 `ObjectOutputStream` 类型清单

**结论**：Redis 场景下常见 JDK 内置序列化类型已覆盖；未特判的冷门类型仍走通用 `$class` + 字段路径。
