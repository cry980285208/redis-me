import json from './commands.json' with { type: 'json' }

// redis仓库查询所有命令, 转换为简单的JSON格式, 用于终端提示
// 说明: 虽然可以直接执行command命令从服务器获取，但国际化需额外处理，因此仍然使用本地数据
// https://github.com/redis/redis/tree/unstable/src/commands
// https://raw.githubusercontent.com/redis/redis-doc/refs/heads/master/commands.json

/*
vue-web-terminal的格式

type Command = {
    key: string;
    title?: string;
    group?: string;
    usage?: string;
    description?: string;
    example?: Array<CommandExample>;
};
*/
const commands = []

// 遍历原始 JSON 对象
for (const [commandName, commandData] of Object.entries(json)) {
  const { group, summary, since } = commandData

  commands.push({
    key: commandName,
    title: commandName,
    group,
    description: summary + ` @since ${since}`,
    usage: '', // 语法格式
  })
}

console.log(JSON.stringify(commands, null, 2))
console.log('命令总数', commands.length)
