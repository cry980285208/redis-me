import _ from 'lodash'

let str = `
id: 唯一的 64 位客户端 ID
addr: 客户端的地址/端口
laddr: 客户端连接到的本地地址/端口（绑定地址）
fd: 对应于套接字的文件描述符
name: 客户端使用 CLIENT SETNAME 设置的名称
age: 连接的总持续时间（秒）
idle: 连接的空闲时间（秒）
flags: 客户端标志（见下文）
db: 当前数据库 ID
sub: 频道订阅数
psub: 模式匹配订阅数
ssub: 分片频道订阅数。在 Redis 7.0.3 中添加
multi: MULTI/EXEC 上下文中的命令数
watch: 此客户端当前正在监视的键数。在 Redis 7.4 中添加
qbuf: 查询缓冲区长度（0 表示没有待处理的查询）
qbuf-free: 查询缓冲区的可用空间（0 表示缓冲区已满）
argv-mem: 下一个命令的不完整参数（已从查询缓冲区中提取）
multi-mem: 缓冲的多命令使用的内存。在 Redis 7.0 中添加
obl: 输出缓冲区长度
oll: 输出列表长度（当缓冲区满时，回复在此列表中排队）
omem: 输出缓冲区内存使用情况
tot-mem: 此客户端在其各种缓冲区中消耗的总内存
events: 文件描述符事件（见下文）
cmd: 执行的最后一条命令
user: 客户端的已认证用户名
redir: 当前客户端跟踪重定向的客户端 id
resp: 客户端 RESP 协议版本。在 Redis 7.0 中添加
rbp: 客户端连接以来其读取缓冲区的峰值大小。在 Redis 7.0 中添加
rbs: 客户端读取缓冲区当前大小（字节）。在 Redis 7.0 中添加
io-thread: 分配给客户端的 I/O 线程 ID。在 Redis 8.0 中添加
`

str = `
id: a unique 64-bit client ID.
addr: address/port of the client.
laddr: address/port of local address client connected to (bind address).
fd: file descriptor corresponding to the socket.
name: the name set by the client with CLIENT SETNAME.
age: total duration of the connection in seconds.
idle: idle time of the connection in seconds.
flags: client flags (see below).
db: current database ID.
sub: number of channel subscriptions.
psub: number of pattern matching subscriptions.
ssub: number of shard channel subscriptions. Added in Redis 7.0.3.
multi: number of commands in a MULTI/EXEC context.
watch: number of keys this client is currently watching. Added in Redis 7.4.
qbuf: query buffer length (0 means no query pending).
qbuf-free: free space of the query buffer (0 means the buffer is full).
argv-mem: incomplete arguments for the next command (already extracted from query buffer).
multi-mem: memory is used up by buffered multi commands. Added in Redis 7.0.
obl: output buffer length.
oll: output list length (replies are queued in this list when the buffer is full).
omem: output buffer memory usage.
tot-mem: total memory consumed by this client in its various buffers.
events: file descriptor events (see below).
cmd: last command played.
user: the authenticated username of the client.
redir: client id of current client tracking redirection.
resp: client RESP protocol version. Added in Redis 7.0.
rbp: peak size of the client's read buffer since the client connected. Added in Redis 7.0.
rbs: current size of the client's read buffer in bytes. Added in Redis 7.0.
lib-name: the name of the client library that is being used.
lib-ver: the version of the client library.
io-thread: id of I/O thread assigned to the client. Added in Redis 8.0
tot-net-in: total network input bytes read from this client.
tot-net-out: total network output bytes sent to this client.
tot-cmds: total count of commands this client executed.
`
// 转换为json格式输出
const json: Record<string, string> = {}
str
  .trim()
  .split('\n')
  .forEach(item => {
    const [key, value] = item.split(': ')
    if (key) {
      json[_.camelCase(key)] = _.trim(value, '.')
    }
  })
console.log(json)
console.log(Object.keys(json))
