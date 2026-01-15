"""
AkShare数据获取模块
负责从AkShare API获取股票数据
"""
import akshare as ak
import pandas as pd
import logging
from datetime import datetime, timedelta

logging.basicConfig(level=logging.INFO, format='%(asctime)s - %(levelname)s - %(message)s')
logger = logging.getLogger(__name__)


class AkShareDataFetcher:
    """AkShare数据获取类"""

    def __init__(self):
        """初始化AkShare API（无需token）"""
        logger.info("AkShare API初始化成功")

    def get_stock_list(self):
        """
        获取A股股票列表

        Returns:
            DataFrame: 股票列表数据
        """
        try:
            # 获取沪深A股实时行情数据（包含股票列表）
            df = ak.stock_zh_a_spot_em()

            # 重命名列以匹配数据库结构
            df_result = pd.DataFrame({
                'ts_code': df['代码'].apply(self._convert_to_ts_code),
                'symbol': df['代码'],
                'name': df['名称'],
                'market': df['代码'].apply(lambda x: self._get_market(x)),
                'list_date': None  # AkShare基础接口不提供上市日期
            })

            logger.info(f"获取股票列表成功，共 {len(df_result)} 只股票")
            return df_result
        except Exception as e:
            logger.error(f"获取股票列表失败: {e}")
            raise

    def get_daily_data(self, symbol, start_date=None, end_date=None, adjust='qfq'):
        """
        获取股票日线数据

        Args:
            symbol: 股票代码，如 '000001' 或 '600000'
            start_date: 开始日期，格式 'YYYYMMDD' 或 'YYYY-MM-DD'
            end_date: 结束日期，格式 'YYYYMMDD' 或 'YYYY-MM-DD'
            adjust: 复权类型，'qfq'=前复权, 'hfq'=后复权, ''=不复权

        Returns:
            DataFrame: 日线数据
        """
        try:
            # 转换日期格式
            if start_date:
                start_date = self._format_date(start_date)
            else:
                start_date = (datetime.now() - timedelta(days=365)).strftime('%Y%m%d')

            if end_date:
                end_date = self._format_date(end_date)
            else:
                end_date = datetime.now().strftime('%Y%m%d')

            # 获取历史行情数据
            df = ak.stock_zh_a_hist(
                symbol=symbol,
                period="daily",
                start_date=start_date,
                end_date=end_date,
                adjust=adjust
            )

            if df is not None and not df.empty:
                # 重命名列以匹配数据库结构
                df_result = pd.DataFrame({
                    'trade_date': pd.to_datetime(df['日期']),
                    'open': df['开盘'],
                    'high': df['最高'],
                    'low': df['最低'],
                    'close': df['收盘'],
                    'volume': df['成交量'],
                    'amount': df['成交额'],
                    'pct_chg': df['涨跌幅']
                })

                # 计算昨收价和涨跌额
                df_result['pre_close'] = df_result['close'].shift(1)
                df_result['change'] = df_result['close'] - df_result['pre_close']

                # 转换成交量单位（手）
                df_result['vol'] = df_result['volume'] / 100

                # 按日期排序
                df_result = df_result.sort_values('trade_date')

                logger.info(f"获取 {symbol} 日线数据成功，共 {len(df_result)} 条记录")
                return df_result
            else:
                logger.warning(f"未获取到 {symbol} 的日线数据")
                return pd.DataFrame()

        except Exception as e:
            logger.error(f"获取 {symbol} 日线数据失败: {e}")
            raise

    def get_realtime_quote(self, symbols=None):
        """
        获取实时行情数据

        Args:
            symbols: 股票代码列表，如 ['000001', '600000']，None表示获取所有

        Returns:
            DataFrame: 实时行情数据
        """
        try:
            # 获取所有A股实时行情
            df = ak.stock_zh_a_spot_em()

            if symbols:
                # 筛选指定股票
                df = df[df['代码'].isin(symbols)]

            if df is not None and not df.empty:
                # 重命名列
                df_result = pd.DataFrame({
                    'symbol': df['代码'],
                    'name': df['名称'],
                    'price': df['最新价'],
                    'pct_chg': df['涨跌幅'],
                    'change': df['涨跌额'],
                    'volume': df['成交量'],
                    'amount': df['成交额'],
                    'open': df['今开'],
                    'high': df['最高'],
                    'low': df['最低'],
                    'pre_close': df['昨收'],
                    'timestamp': datetime.now()
                })

                logger.info(f"获取实时行情成功，共 {len(df_result)} 只股票")
                return df_result
            else:
                logger.warning("未获取到实时行情数据")
                return pd.DataFrame()

        except Exception as e:
            logger.error(f"获取实时行情失败: {e}")
            raise

    def get_stock_info(self, symbol):
        """
        获取股票详细信息

        Args:
            symbol: 股票代码，如 '000001'

        Returns:
            dict: 股票信息
        """
        try:
            # 获取个股信息
            df = ak.stock_individual_info_em(symbol=symbol)

            if df is not None and not df.empty:
                info = {}
                for _, row in df.iterrows():
                    info[row['item']] = row['value']

                logger.info(f"获取 {symbol} 股票信息成功")
                return info
            else:
                logger.warning(f"未获取到 {symbol} 的股票信息")
                return {}

        except Exception as e:
            logger.error(f"获取 {symbol} 股票信息失败: {e}")
            return {}

    def get_stock_board(self):
        """
        获取股票板块分类

        Returns:
            DataFrame: 板块数据
        """
        try:
            # 获取东方财富板块行情
            df = ak.stock_board_industry_name_em()
            logger.info(f"获取板块数据成功，共 {len(df)} 个板块")
            return df
        except Exception as e:
            logger.error(f"获取板块数据失败: {e}")
            raise

    def _convert_to_ts_code(self, symbol):
        """
        将股票代码转换为TS格式（如 000001.SZ）

        Args:
            symbol: 股票代码，如 '000001'

        Returns:
            str: TS格式代码
        """
        if symbol.startswith('6'):
            return f"{symbol}.SH"
        elif symbol.startswith(('0', '3')):
            return f"{symbol}.SZ"
        elif symbol.startswith('8') or symbol.startswith('4'):
            return f"{symbol}.BJ"
        else:
            return f"{symbol}.SZ"

    def _get_market(self, symbol):
        """
        根据代码判断市场

        Args:
            symbol: 股票代码

        Returns:
            str: 市场名称
        """
        if symbol.startswith('6'):
            return '上交所'
        elif symbol.startswith('0'):
            return '深交所主板'
        elif symbol.startswith('3'):
            return '深交所创业板'
        elif symbol.startswith('8') or symbol.startswith('4'):
            return '北交所'
        else:
            return '未知'

    def _format_date(self, date_str):
        """
        格式化日期字符串为YYYYMMDD格式

        Args:
            date_str: 日期字符串，支持 'YYYYMMDD' 或 'YYYY-MM-DD'

        Returns:
            str: YYYYMMDD格式日期
        """
        if '-' in date_str:
            return date_str.replace('-', '')
        return date_str
