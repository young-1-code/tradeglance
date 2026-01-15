"""
数据库管理模块
负责数据库连接、表创建和管理
"""
import pymysql
from sqlalchemy import create_engine, text
from sqlalchemy.pool import QueuePool
import logging
import sys
import os

# 添加父目录到路径以便导入config
sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from config.config import DB_CONFIG, DATABASE_URL

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


class DatabaseManager:
    """数据库管理类"""

    def __init__(self):
        self.db_config = DB_CONFIG
        self.engine = None
        self._init_database()
        self._init_engine()

    def _init_database(self):
        """初始化数据库（如果不存在则创建）"""
        try:
            # 连接MySQL服务器（不指定数据库）
            conn = pymysql.connect(
                host=self.db_config['host'],
                port=self.db_config['port'],
                user=self.db_config['user'],
                password=self.db_config['password'],
                charset=self.db_config['charset']
            )
            cursor = conn.cursor()

            # 创建数据库
            cursor.execute(f"CREATE DATABASE IF NOT EXISTS {self.db_config['database']} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci")
            logger.info(f"数据库 {self.db_config['database']} 已就绪")

            cursor.close()
            conn.close()
        except Exception as e:
            logger.error(f"初始化数据库失败: {e}")
            raise

    def _init_engine(self):
        """初始化SQLAlchemy引擎"""
        try:
            self.engine = create_engine(
                DATABASE_URL,
                poolclass=QueuePool,
                pool_size=10,
                max_overflow=20,
                pool_pre_ping=True,
                echo=False
            )
            logger.info("数据库引擎初始化成功")
        except Exception as e:
            logger.error(f"初始化数据库引擎失败: {e}")
            raise

    def create_daily_table(self, stock_code):
        """
        创建股票日线数据表

        Args:
            stock_code: 股票代码，如 '000001.SZ'
        """
        # 将股票代码转换为表名（替换.为_）
        table_name = f"daily_{stock_code.replace('.', '_')}"

        create_table_sql = f"""
        CREATE TABLE IF NOT EXISTS `{table_name}` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `trade_date` DATE NOT NULL COMMENT '交易日期',
            `open` DECIMAL(10, 2) COMMENT '开盘价',
            `high` DECIMAL(10, 2) COMMENT '最高价',
            `low` DECIMAL(10, 2) COMMENT '最低价',
            `close` DECIMAL(10, 2) COMMENT '收盘价',
            `pre_close` DECIMAL(10, 2) COMMENT '昨收价',
            `change` DECIMAL(10, 2) COMMENT '涨跌额',
            `pct_chg` DECIMAL(10, 4) COMMENT '涨跌幅(%)',
            `vol` DECIMAL(20, 2) COMMENT '成交量(手)',
            `amount` DECIMAL(20, 2) COMMENT '成交额(千元)',
            `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            UNIQUE KEY `idx_trade_date` (`trade_date`),
            KEY `idx_created_at` (`created_at`)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='股票日线数据-{stock_code}'
        """

        try:
            with self.engine.connect() as conn:
                conn.execute(text(create_table_sql))
                conn.commit()
            logger.info(f"日线数据表 {table_name} 创建成功")
            return table_name
        except Exception as e:
            logger.error(f"创建日线数据表失败: {e}")
            raise

    def create_realtime_table(self, stock_code):
        """
        创建股票实时数据表

        Args:
            stock_code: 股票代码，如 '000001.SZ'
        """
        # 将股票代码转换为表名（替换.为_）
        table_name = f"realtime_{stock_code.replace('.', '_')}"

        create_table_sql = f"""
        CREATE TABLE IF NOT EXISTS `{table_name}` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `timestamp` DATETIME NOT NULL COMMENT '时间戳',
            `price` DECIMAL(10, 2) COMMENT '当前价',
            `open` DECIMAL(10, 2) COMMENT '开盘价',
            `high` DECIMAL(10, 2) COMMENT '最高价',
            `low` DECIMAL(10, 2) COMMENT '最低价',
            `pre_close` DECIMAL(10, 2) COMMENT '昨收价',
            `volume` DECIMAL(20, 2) COMMENT '成交量',
            `amount` DECIMAL(20, 2) COMMENT '成交额',
            `bid1` DECIMAL(10, 2) COMMENT '买一价',
            `bid1_volume` DECIMAL(20, 2) COMMENT '买一量',
            `ask1` DECIMAL(10, 2) COMMENT '卖一价',
            `ask1_volume` DECIMAL(20, 2) COMMENT '卖一量',
            `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            KEY `idx_timestamp` (`timestamp`),
            KEY `idx_created_at` (`created_at`)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='股票实时数据-{stock_code}'
        """

        try:
            with self.engine.connect() as conn:
                conn.execute(text(create_table_sql))
                conn.commit()
            logger.info(f"实时数据表 {table_name} 创建成功")
            return table_name
        except Exception as e:
            logger.error(f"创建实时数据表失败: {e}")
            raise

    def create_stock_list_table(self):
        """创建股票列表表"""
        create_table_sql = """
        CREATE TABLE IF NOT EXISTS `stock_list` (
            `id` INT AUTO_INCREMENT PRIMARY KEY,
            `ts_code` VARCHAR(20) NOT NULL COMMENT 'TS代码',
            `symbol` VARCHAR(10) COMMENT '股票代码',
            `name` VARCHAR(50) COMMENT '股票名称',
            `area` VARCHAR(20) COMMENT '地域',
            `industry` VARCHAR(50) COMMENT '所属行业',
            `market` VARCHAR(10) COMMENT '市场类型',
            `list_date` DATE COMMENT '上市日期',
            `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            `updated_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            UNIQUE KEY `idx_ts_code` (`ts_code`)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='股票列表'
        """

        try:
            with self.engine.connect() as conn:
                conn.execute(text(create_table_sql))
                conn.commit()
            logger.info("股票列表表创建成功")
        except Exception as e:
            logger.error(f"创建股票列表表失败: {e}")
            raise

    def get_connection(self):
        """获取数据库连接"""
        return self.engine.connect()

    def close(self):
        """关闭数据库连接"""
        if self.engine:
            self.engine.dispose()
            logger.info("数据库连接已关闭")
