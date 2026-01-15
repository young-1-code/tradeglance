"""
数据存储模块
负责将获取的股票数据存储到MySQL数据库
"""
import pandas as pd
import logging
from sqlalchemy import text
from datetime import datetime
import sys
import os

# 添加父目录到路径以便导入其他模块
sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from src.db_manager import DatabaseManager
from src.akshare_fetcher import AkShareDataFetcher

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


class StockDataStorage:
    """股票数据存储类"""

    def __init__(self):
        """初始化数据存储"""
        self.db_manager = DatabaseManager()
        self.fetcher = AkShareDataFetcher()
        logger.info("数据存储模块初始化成功")

    def save_stock_list(self):
        """
        获取并保存股票列表到数据库
        """
        try:
            # 创建股票列表表
            self.db_manager.create_stock_list_table()

            # 获取股票列表
            df = self.fetcher.get_stock_list()

            if df.empty:
                logger.warning("股票列表为空")
                return

            # 保存到数据库
            with self.db_manager.get_connection() as conn:
                df.to_sql(
                    'stock_list',
                    con=conn,
                    if_exists='replace',
                    index=False,
                    method='multi',
                    chunksize=1000
                )
                conn.commit()

            logger.info(f"股票列表保存成功，共 {len(df)} 只股票")
            return df

        except Exception as e:
            logger.error(f"保存股票列表失败: {e}")
            raise

    def save_daily_data(self, ts_code, start_date=None, end_date=None):
        """
        获取并保存股票日线数据

        Args:
            ts_code: 股票代码，如 '000001.SZ' 或 '000001'
            start_date: 开始日期，格式 'YYYYMMDD'
            end_date: 结束日期，格式 'YYYYMMDD'
        """
        try:
            # 创建日线数据表
            table_name = self.db_manager.create_daily_table(ts_code)

            # 提取纯股票代码（去掉.SZ/.SH后缀）
            symbol = ts_code.split('.')[0] if '.' in ts_code else ts_code

            # 获取日线数据
            df = self.fetcher.get_daily_data(symbol, start_date, end_date)

            if df.empty:
                logger.warning(f"{ts_code} 日线数据为空")
                return

            # 辅助函数：将NaN转换为None
            def to_db_value(val):
                if pd.isna(val):
                    return None
                return val

            # 保存到数据库
            with self.db_manager.get_connection() as conn:
                # 使用replace模式，如果数据已存在则更新
                for _, row in df.iterrows():
                    insert_sql = text(f"""
                        INSERT INTO `{table_name}`
                        (trade_date, open, high, low, close, pre_close, `change`, pct_chg, vol, amount)
                        VALUES
                        (:trade_date, :open, :high, :low, :close, :pre_close, :change, :pct_chg, :vol, :amount)
                        ON DUPLICATE KEY UPDATE
                        open=VALUES(open), high=VALUES(high), low=VALUES(low),
                        close=VALUES(close), pre_close=VALUES(pre_close),
                        `change`=VALUES(`change`), pct_chg=VALUES(pct_chg),
                        vol=VALUES(vol), amount=VALUES(amount)
                    """)

                    conn.execute(insert_sql, {
                        'trade_date': to_db_value(row['trade_date']),
                        'open': to_db_value(row['open']),
                        'high': to_db_value(row['high']),
                        'low': to_db_value(row['low']),
                        'close': to_db_value(row['close']),
                        'pre_close': to_db_value(row['pre_close']),
                        'change': to_db_value(row['change']),
                        'pct_chg': to_db_value(row['pct_chg']),
                        'vol': to_db_value(row['vol']),
                        'amount': to_db_value(row['amount'])
                    })

                conn.commit()

            logger.info(f"{ts_code} 日线数据保存成功，共 {len(df)} 条记录")

        except Exception as e:
            logger.error(f"保存 {ts_code} 日线数据失败: {e}")
            raise

    def save_realtime_data(self, ts_code):
        """
        获取并保存股票实时数据

        Args:
            ts_code: 股票代码，如 '000001.SZ' 或 '000001'
        """
        try:
            # 创建实时数据表
            table_name = self.db_manager.create_realtime_table(ts_code)

            # 提取纯股票代码
            symbol = ts_code.split('.')[0] if '.' in ts_code else ts_code

            # 获取实时数据
            df = self.fetcher.get_realtime_quote([symbol])

            if df.empty:
                logger.warning(f"{ts_code} 实时数据为空")
                return

            # 取第一条记录
            row = df.iloc[0]

            # 插入数据
            with self.db_manager.get_connection() as conn:
                insert_sql = text(f"""
                    INSERT INTO `{table_name}`
                    (timestamp, price, open, high, low, pre_close, volume, amount)
                    VALUES
                    (:timestamp, :price, :open, :high, :low, :pre_close, :volume, :amount)
                """)

                conn.execute(insert_sql, {
                    'timestamp': row['timestamp'],
                    'price': row['price'],
                    'open': row['open'],
                    'high': row['high'],
                    'low': row['low'],
                    'pre_close': row['pre_close'],
                    'volume': row['volume'],
                    'amount': row['amount']
                })
                conn.commit()

            logger.info(f"{ts_code} 实时数据保存成功")

        except Exception as e:
            logger.error(f"保存 {ts_code} 实时数据失败: {e}")
            raise

    def batch_save_daily_data(self, ts_codes, start_date=None, end_date=None):
        """
        批量保存多只股票的日线数据

        Args:
            ts_codes: 股票代码列表
            start_date: 开始日期，格式 'YYYYMMDD'
            end_date: 结束日期，格式 'YYYYMMDD'
        """
        success_count = 0
        fail_count = 0

        for ts_code in ts_codes:
            try:
                self.save_daily_data(ts_code, start_date, end_date)
                success_count += 1
            except Exception as e:
                logger.error(f"保存 {ts_code} 失败: {e}")
                fail_count += 1
                continue

        logger.info(f"批量保存完成: 成功 {success_count} 只，失败 {fail_count} 只")

    def get_stock_codes_from_db(self):
        """
        从数据库获取股票代码列表

        Returns:
            list: 股票代码列表
        """
        try:
            with self.db_manager.get_connection() as conn:
                result = conn.execute(text("SELECT ts_code FROM stock_list"))
                codes = [row[0] for row in result]
                return codes
        except Exception as e:
            logger.error(f"从数据库获取股票代码失败: {e}")
            return []

    def close(self):
        """关闭数据库连接"""
        self.db_manager.close()
