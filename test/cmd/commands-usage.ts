// 读取redis docs目录下已经生成的markdown命令文档, 并获取命令用法
// 位置: https://github.com/redis/docs
// 目录: content/commands/
// 示例: acl-cat.md
//    title: ACL CAT
//    syntax_fmt: ACL CAT [category]
//    summary: Lists the ACL categories, or the commands inside a category.
//    group: server
//    since: 6.0.0

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

import fs from 'fs'

import frontMatter from 'front-matter'

interface CmdMdAttributes {
  title?: string
  syntax_fmt?: string
  summary?: string
  group?: string
  since?: string
}

const commands: {
  key: string
  title: string
  group: string
  summary: string
  since: string
  usage: string
}[] = []
const summarys = []
const dir = 'C:/Users/he_pe/Desktop/work/docs/content/commands/'
fs.readdirSync(dir).forEach(file => {
  if (file.endsWith('.md')) {
    const content = fs.readFileSync(dir + file, 'utf8')
    const parsed = frontMatter(content)
    const { title, syntax_fmt, summary, group, since } = parsed.attributes as CmdMdAttributes

    if (!title || !syntax_fmt || !summary || !group || !since) {
      console.log('缺少属性', file)
      return
    }

    commands.push({ key: title, title, group, summary, since, usage: syntax_fmt })

    summarys.push(summary)
  }
})

// 将命令保存为json文件
//fs.writeFileSync('commands-usage.json', JSON.stringify(commands, null, 2))
// fs.writeFileSync('commands-summarys.json', JSON.stringify(summarys, null, 2))
console.log('命令总数', commands.length)
