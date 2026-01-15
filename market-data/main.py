#!/usr/bin/env python3
"""
股票数据入库主程序
将股票日线数据和实时行情数据写入MySQL数据库
"""
import sys
import os
import argparse
import time

sys.path.append(os.path.dirname(os.path.abspath(__file__)))

from src.data_storage import StockDataStorage
import logging

logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


def save_stock_list(storage):
    """保存股票列表"""
    logger.info("开始保存股票列表...")
    df = storage.save_stock_list()
    if df is not None:
        logger.info(f"股票列表保存完成，共 {len(df)} 只股票")
    return df


def save_daily_data(storage, codes, start_date=None, end_date=None):
    """保存日线数据"""
    logger.info(f"开始保存日线数据，共 {len(codes)} 只股票...")
    storage.batch_save_daily_data(codes, start_date, end_date)
    logger.info("日线数据保存完成")


def save_realtime_data(storage, codes):
    """保存实时行情数据"""
    logger.info(f"开始保存实时行情数据，共 {len(codes)} 只股票...")
    success = 0
    fail = 0
    for code in codes:
        try:
            storage.save_realtime_data(code)
            success += 1
            # 避免请求过快
            time.sleep(0.1)
        except Exception as e:
            logger.error(f"保存 {code} 实时数据失败: {e}")
            fail += 1
    logger.info(f"实时行情保存完成: 成功 {success}，失败 {fail}")


def main():
    parser = argparse.ArgumentParser(description='股票数据入库程序')
    parser.add_argument('--mode', choices=['all', 'list', 'daily', 'realtime'],
                        default='all', help='运行模式: all=全部, list=股票列表, daily=日线, realtime=实时')
    parser.add_argument('--codes', nargs='+', help='指定股票代码列表，如: 000001.SZ 600000.SH')
    parser.add_argument('--start', help='日线数据开始日期，格式: YYYYMMDD')
    parser.add_argument('--end', help='日线数据结束日期，格式: YYYYMMDD')
    parser.add_argument('--limit', type=int, default=0, help='限制处理的股票数量，0表示不限制')

    args = parser.parse_args()

    storage = None
    try:
        storage = StockDataStorage()

        # 获取股票代码列表
        if args.codes:
            codes = args.codes
        else:
            # 先保存股票列表
            df = save_stock_list(storage)
            if df is not None:
                codes = df['ts_code'].tolist()
                if args.limit > 0:
                    codes = codes[:args.limit]
            else:
                codes = []

        if not codes:
            logger.warning("没有股票代码可处理")
            return

        logger.info(f"待处理股票数量: {len(codes)}")

        # 根据模式执行
        if args.mode == 'all':
            save_daily_data(storage, codes, args.start, args.end)
            save_realtime_data(storage, codes)
        elif args.mode == 'list':
            # 已经在上面保存了
            pass
        elif args.mode == 'daily':
            save_daily_data(storage, codes, args.start, args.end)
        elif args.mode == 'realtime':
            save_realtime_data(storage, codes)

        logger.info("数据入库完成!")

    except Exception as e:
        logger.error(f"程序执行失败: {e}")
        raise
    finally:
        if storage:
            storage.close()


if __name__ == '__main__':
    main()
