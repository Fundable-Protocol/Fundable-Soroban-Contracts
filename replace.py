import re

with open("contracts/lockup/src/test.rs", "r") as f:
    content = f.read()

pattern = r"let stream_id = client\.create\(\s*&sender,\s*&recipient,\s*&token,\s*&?\((.*?)\),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?\((.*?)\),\s*&?\((.*?)\),\s*&?(.*?),\s*&?(.*?),\s*\);"

def repl(m):
    total = m.group(1).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    start = m.group(2).replace("u64", "").replace("i128", "")
    end = m.group(3).replace("u64", "").replace("i128", "")
    cliff = m.group(4).replace("u64", "").replace("i128", "")
    start_unlock = m.group(5).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    cliff_unlock = m.group(6).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    granularity = m.group(7).replace("u64", "").replace("i128", "")
    cancelable = m.group(8).replace("u64", "").replace("i128", "")
    
    return f"""let params = CreateLockupParams {{
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: {total},
        start_time: {start},
        end_time: {end},
        cliff_time: {cliff},
        start_unlock_amount: {start_unlock},
        cliff_unlock_amount: {cliff_unlock},
        granularity: {granularity},
        cancelable: {cancelable},
    }};
    let stream_id = client.create(&params);"""

new_content = re.sub(pattern, repl, content)

# A second pattern for where it is simply `&total` instead of `&(total)`
pattern2 = r"let stream_id = client\.create\(\s*&sender,\s*&recipient,\s*&token,\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*&?(.*?),\s*\);"

def repl2(m):
    total = m.group(1).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    start = m.group(2).replace("u64", "").replace("i128", "")
    end = m.group(3).replace("u64", "").replace("i128", "")
    cliff = m.group(4).replace("u64", "").replace("i128", "")
    start_unlock = m.group(5).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    cliff_unlock = m.group(6).replace("u64", "").replace("i128", "").replace(" ", "").replace("*", " * ")
    granularity = m.group(7).replace("u64", "").replace("i128", "")
    cancelable = m.group(8).replace("u64", "").replace("i128", "")
    
    return f"""let params = CreateLockupParams {{
        sender: sender.clone(),
        recipient: recipient.clone(),
        token: token.clone(),
        total_amount: {total},
        start_time: {start},
        end_time: {end},
        cliff_time: {cliff},
        start_unlock_amount: {start_unlock},
        cliff_unlock_amount: {cliff_unlock},
        granularity: {granularity},
        cancelable: {cancelable},
    }};
    let stream_id = client.create(&params);"""

new_content2 = re.sub(pattern2, repl2, new_content)

with open("contracts/lockup/src/test.rs", "w") as f:
    f.write(new_content2)
