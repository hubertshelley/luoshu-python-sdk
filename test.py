import threading
from time import sleep

import luoshu_python_sdk
import asyncio

luoshu = luoshu_python_sdk.Luoshu(
    "default",
    "test",
    "127.0.0.1",
    8000,
    "192.168.110.103",
)


# luoshu.config_subscribe("default|test", lambda x: print(x))


async def process():
    print("process")
    await luoshu.process()


async def config_subscribe(config_name, namespace="default"):
    while True:
        x = await luoshu.config_subscribe(namespace, config_name)
        print(x)


# def config_subscribe():
#     print("config_subscribe")
#     t = threading.Thread(target=luoshu.config_subscribe, args=("default|test", lambda x: print(x)))
#     t.setDaemon(True)
#     t.start()


def async_running():
    print("async_running")
    t = threading.Thread(target=lambda: asyncio.run(process()))
    t.setDaemon(True)
    t.start()


def async_config_subscribe(config_name, namespace="default"):
    print("async_running")
    t = threading.Thread(target=lambda: asyncio.run(config_subscribe(config_name=config_name, namespace=namespace)))
    t.setDaemon(True)
    t.start()


if __name__ == "__main__":
    # result = luoshu.sum_as_string(1, 2)
    # print(result)
    # config_subscribe()
    async_running()
    sleep(1)
    # luoshu.test_subscribe("default|test")
    # t = threading.Thread(target=lambda: luoshu.config_subscribe("default", "test", lambda x: print("python recv", x)))
    # t.setDaemon(True)
    # t.start()
    async_config_subscribe("test")

    while True:
        sleep(1)
